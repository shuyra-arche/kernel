//! Kernel-retained external address projections. The records join a Pend borrow
//! to a Nest IoSpace completion; they contain no device ownership or BAR operations.
use crate::basis::flat::FlatRange;
use crate::basis::pend;
use crate::basis::pend::ExternalBorrow;
use crate::basis::pend::Hold;
use crate::basis::pend::Need;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::life::Life;
use crate::basis::unit::task::Task;
use crate::machine::iommu::BindingId;
use crate::machine::iommu::Error;
use crate::machine::iommu::Phase;
use crate::machine::iommu::riscv::aperture;
use crate::machine::iommu::riscv::retirement;
use crate::machine::sync::SpinLock;
use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use kernel_api::abi::call::cell::CellFail;
use kernel_api::model::cell::MappingId;
use kernel_api::model::cell::MappingInfo;
use kernel_api::model::cell::MappingState;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::modifiable::Modifiable;

struct Projection {
    id: usize,
    actor: Weak<Task>,
    task: TaskId,
    source: FlatRange,
    target: Option<pend::PendSnapshot>,
    platform: aperture::Binding,
    binding: Option<BindingId>,
    borrow: Option<ExternalBorrow>,
    info: MappingInfo,
    events: Arc<Life>,
}
static PROJECTIONS: SpinLock<Vec<Projection>> = SpinLock::new(Vec::new());
static NEXT: AtomicUsize = AtomicUsize::new(1);
fn failure(error: Error) -> CellFail {
    match error {
        Error::OutOfMemory | Error::Capacity => CellFail::OoM,
        Error::Overlap => CellFail::AlreadyMapped,
        Error::InvalidRange => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}
fn target(actor: &Task, token: PendId, write: bool) -> Result<pend::PendSnapshot, CellFail> {
    let entry = pend::locate(actor, token).ok_or(CellFail::Denied)?;
    if !entry.alive() || (write && !entry.allows(Need::Write)) {
        return Err(CellFail::Denied);
    }
    let view = entry.view().share_view().ok_or(CellFail::Denied)?;
    if let Some((offset, count)) = entry.authority().window() {
        if offset.get() != 0 || count != view.source().page_count() {
            return Err(CellFail::Denied);
        }
    }
    Ok(entry)
}
fn authorized<'a>(
    records: &'a mut [Projection],
    actor: &Task,
    token: PendId,
    id: MappingId,
) -> Result<&'a mut Projection, CellFail> {
    let entry = target(actor, token, false)?;
    let range = entry.view().share_view().ok_or(CellFail::Denied)?.source();
    records
        .iter_mut()
        .find(|p| p.id == id.get() && p.source.same_range(&range))
        .ok_or(CellFail::Denied)
}
/// Caller holds Task serialization and work commit. All metadata is reserved
/// before publishing a hardware relation; failures retain the prepared record.
pub(crate) fn attach(
    actor: &Arc<Task>,
    token: PendId,
    hold: &Hold,
    iova: usize,
    modifiable: bool,
    change: Option<&Arc<crate::basis::cell::page::Change>>,
) -> Result<usize, CellFail> {
    if iova == 0 || iova >= 1usize << 38 || !iova.is_multiple_of(4096) {
        return Err(CellFail::NotAligned);
    }
    let target = target(actor, token, true)?;
    if target.modifiable() != Modifiable::Yes {
        return Err(CellFail::Denied);
    }
    let target_view = target.view().share_view().ok_or(CellFail::Denied)?;
    // The same View transaction excludes Narrow/Accord's CPU-withdrawal phase.
    // A failed routing check must not silently remove the driver's BAR mapping.
    let _target_operation = crate::basis::view::share_view::ShareOperation::acquire(&target_view)
        .ok_or(CellFail::Busy)?;
    let source = target_view.source();
    let borrow = ExternalBorrow::prepare(hold, actor, modifiable, change.map(Arc::as_ref))
        .map_err(|e| match e {
            kernel_api::abi::call::pend::PendFail::Busy => CellFail::Busy,
            kernel_api::abi::call::pend::PendFail::OoM => CellFail::OoM,
            _ => CellFail::Denied,
        })?;
    let events = Life::try_new().map_err(|_| CellFail::OoM)?;
    let mut records = PROJECTIONS.lock();
    records.try_reserve(1).map_err(|_| CellFail::OoM)?;
    let id = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            id.checked_add(1).filter(|id| *id <= isize::MAX as usize)
        })
        .map_err(|_| CellFail::OoM)?;
    target
        .authority()
        .retain_route()
        .map_err(|_| CellFail::Busy)?;
    let platform = match aperture::bind(&source, &actor.ident.team.space.dma) {
        Ok(platform) => platform,
        Err(error) => {
            unsafe { target.authority().release_route() };
            return Err(failure(error));
        }
    };
    records.push(Projection {
        id,
        actor: Arc::downgrade(actor),
        task: actor.ident.id,
        source,
        target: Some(target),
        platform,
        binding: None,
        borrow: Some(borrow),
        info: MappingInfo {
            state: MappingState::Publishing,
            fault_cause: 0,
            fault_address: 0,
        },
        events,
    });
    let record = records.last_mut().unwrap();
    let result = retirement::with_space(
        &actor.ident.team.space.dma,
        record.platform.domain,
        |space| {
            let borrow = record.borrow.as_mut().unwrap();
            // SAFETY: exact source is retained and borrowed, and the target Pend
            // resolves to a platform-validated route with protected configuration.
            let binding = unsafe {
                match change {
                    Some(change) => {
                        space.register_during(borrow.cell(), iova, modifiable, Some(change))
                    }
                    None => space.register(borrow.cell(), iova, modifiable),
                }
            }?;
            record.binding = Some(binding);
            borrow.publish();
            space.publish(binding)
        },
    );
    if let Err(error) = result {
        close(record);
        return Err(failure(error));
    }
    Ok(id)
}
fn close(record: &mut Projection) {
    if record.info.state == MappingState::Settled {
        return;
    }
    let stopped = aperture::stop(record.platform.port);
    let closing = retirement::close(record.platform.domain);
    record.info.state = if stopped.is_ok() && closing.is_ok() {
        MappingState::Retiring
    } else {
        MappingState::Failed
    };
}
pub(crate) fn detach(actor: &Task, token: PendId, id: MappingId) -> Result<(), CellFail> {
    let mut records = PROJECTIONS.lock();
    let record = authorized(&mut records, actor, token, id)?;
    // Observing a projection is allowed through its parent; initiating withdrawal
    // requires the creator or a live, writable target Pend.
    let creator = record
        .actor
        .upgrade()
        .is_some_and(|a| a.ident.id == actor.ident.id);
    if !creator {
        target(actor, token, true)?;
    }
    close(record);
    Ok(())
}
pub(crate) fn inspect(actor: &Task, token: PendId, id: MappingId) -> Result<MappingInfo, CellFail> {
    let mut records = PROJECTIONS.lock();
    Ok(authorized(&mut records, actor, token, id)?.info)
}
pub(crate) fn event(
    actor: &Task,
    token: PendId,
    id: MappingId,
    fault: bool,
) -> Result<(Weak<Life>, Option<bool>), CellFail> {
    let mut records = PROJECTIONS.lock();
    let record = authorized(&mut records, actor, token, id)?;
    let ready = if record.info.state == MappingState::Failed {
        Some(false)
    } else if fault {
        (record.info.fault_cause != 0).then_some(true)
    } else {
        (record.info.state == MappingState::Settled).then_some(true)
    };
    Ok((Arc::downgrade(&record.events), ready))
}
fn advance(record: &mut Projection) -> [Option<(TaskId, PendId)>; 2] {
    if record.info.state == MappingState::Settled {
        return [None, None];
    }
    let actor = record.actor.upgrade();
    let invalid = actor
        .as_ref()
        .is_none_or(|a| a.dead() || a.ident.team.space.dma.closed())
        || record
            .target
            .as_ref()
            .is_none_or(|t| !t.alive() || !t.allows(Need::Write))
        || record.borrow.as_ref().is_some_and(ExternalBorrow::revoked);
    if invalid || record.info.state == MappingState::Failed {
        close(record);
    }
    if matches!(
        record.info.state,
        MappingState::Publishing | MappingState::Active
    ) {
        for _ in 0..4 {
            match retirement::fault(record.platform.domain) {
                Ok(Some(fault)) if fault.device == record.platform.did => {
                    if record.info.fault_cause == 0 {
                        record.info.fault_cause = fault.cause as usize;
                        record.info.fault_address = fault.address as usize;
                    }
                }
                Ok(None) => break,
                _ => {
                    close(record);
                    break;
                }
            }
        }
    }
    if record.info.state == MappingState::Publishing {
        if let (Some(actor), Some(binding)) = (actor.as_ref(), record.binding) {
            let ready = retirement::with_space(
                &actor.ident.team.space.dma,
                record.platform.domain,
                |space| match space.phase(binding)? {
                    Phase::Publishing => space.ready(binding),
                    Phase::Active => Ok(true),
                    _ => Err(Error::InvalidState),
                },
            );
            match ready {
                Ok(true) => match aperture::activate(record.platform.port) {
                    Ok(()) => record.info.state = MappingState::Active,
                    Err(_) => close(record),
                },
                Ok(false) => {}
                Err(_) => close(record),
            }
        }
    }
    if record.platform.completion.settled() {
        // IOMMU retirement and platform quiescence, not an interrupt count,
        // discharge this exact Pend/Page obligation.
        let observer = record
            .borrow
            .take()
            .and_then(|borrow| unsafe { borrow.settle() });
        let route = record.target.take().map(|target| {
            unsafe { target.authority().release_route() };
            (record.task, target.token())
        });
        record.info.state = MappingState::Settled;
        return [observer, route];
    }
    if retirement::error(record.platform.domain).is_ok_and(|e| e.is_some()) {
        record.info.state = MappingState::Failed;
    }
    [None, None]
}
pub(crate) fn maintain() -> bool {
    crate::basis::cell::changes::maintain();
    let mut pending = retirement::progress();
    static CURSOR: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..4 {
        let (id, changed, observer) = {
            let _commit = crate::basis::commit();
            let Some(mut records) = PROJECTIONS.try_lock() else {
                return true;
            };
            if records.is_empty() {
                break;
            }
            let at = CURSOR.fetch_add(1, Ordering::Relaxed) % records.len();
            let record = &mut records[at];
            let before = record.info;
            let observer = advance(record);
            pending |= record.info.state != MappingState::Settled;
            (record.id, before != record.info, observer)
        };
        for (task, token) in observer.into_iter().flatten() {
            pend::notify(task, token);
        }
        if changed {
            sign::signal(WakeKey::ExternalMapping { id });
        }
    }
    crate::machine::iommu::riscv::interrupt::rearm();
    pending
}
