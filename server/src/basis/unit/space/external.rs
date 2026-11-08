#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum MappingState {
    Publishing = 1,
    Active = 2,
    Retiring = 3,
    Settled = 4,
    Failed = 5,
}
/// Retained I/O projection state. A reported fault never proves reclamation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MappingInfo {
    state: MappingState,
    fault_cause: usize,
    fault_address: usize,
}

// Kernel-retained external address projections. The records join a Pend borrow
// to a Nest IoSpace completion; they contain no device ownership or BAR operations.
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
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::modifiable::Modifiable;

struct Projection {
    id: usize,
    actor: Weak<Task>,
    task: TaskId,
    source: FlatRange,
    target: Option<pend::PendSnapshot>,
    platform: Option<aperture::Binding>,
    hold: Arc<Hold>,
    page: Option<Arc<crate::basis::cell::page::Page>>,
    generation: u64,
    freeze: Option<crate::basis::cell::page::Freeze>,
    iova: usize,
    modifiable: bool,
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
pub(crate) fn create(
    actor: &Arc<Task>,
    token: PendId,
    hold: Arc<Hold>,
    iova: usize,
    modifiable: bool,
) -> Result<usize, CellFail> {
    if iova == 0 || iova >= 1usize << 38 || !iova.is_multiple_of(4096) {
        return Err(CellFail::NotAligned);
    }
    let target = target(actor, token, true)?;
    if target.modifiable() != Modifiable::Yes
        || hold
            .resolve()
            .is_none_or(|r| r.page_count().get() != 1 || r.is_borrowed())
    {
        return Err(CellFail::Denied);
    }
    let source = target.view().share_view().ok_or(CellFail::Denied)?.source();
    let events = Life::try_new().map_err(|_| CellFail::OoM)?;
    let mut records = PROJECTIONS.lock();
    records.try_reserve(1).map_err(|_| CellFail::OoM)?;
    let id = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            id.checked_add(1).filter(|id| *id <= isize::MAX as usize)
        })
        .map_err(|_| CellFail::OoM)?;
    records.push(Projection {
        id,
        actor: Arc::downgrade(actor),
        task: actor.ident.id,
        source,
        target: None,
        platform: None,
        binding: None,
        borrow: None,
        info: MappingInfo {
            state: MappingState::Settled,
            fault_cause: 0,
            fault_address: 0,
        },
        events,
        hold,
        page: None,
        generation: 1,
        freeze: None,
        iova,
        modifiable,
    });
    Ok(id)
}
pub(crate) fn associate(
    actor: &Task,
    token: PendId,
    id: MappingId,
    expected: u64,
    source: &FlatRange,
    page: Arc<crate::basis::cell::page::Page>,
) -> Result<(), CellFail> {
    let mut records = PROJECTIONS.lock();
    let record = authorized(&mut records, actor, token, id)?;
    if record.task != actor.ident.id || !record.hold.resolve().is_some_and(|r| r.same_range(source))
    {
        return Err(CellFail::Denied);
    }
    if record.generation != expected || record.info.state != MappingState::Settled {
        return Err(CellFail::Busy);
    }
    let next = record.generation.checked_add(1).ok_or(CellFail::OoM)?;
    record.page = Some(page);
    record.freeze = None;
    record.generation = next;
    Ok(())
}
pub(crate) fn snapshot(
    actor: &Task,
    token: PendId,
    id: MappingId,
) -> Result<kernel_api::model::cell::CellInfo, CellFail> {
    use kernel_api::model::cell::{CellInfo, CellState};
    let mut records = PROJECTIONS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    Ok(CellInfo {
        generation: r.generation,
        state: match r.info.state {
            MappingState::Publishing => CellState::Publishing,
            MappingState::Active => CellState::Published,
            MappingState::Retiring => CellState::Retracting,
            MappingState::Failed => CellState::Failed,
            MappingState::Settled => {
                if r.page.is_some() {
                    CellState::Ready
                } else {
                    CellState::Vacant
                }
            }
        },
        page: r.page.as_ref().map_or(0, |p| p.id().get() as u64),
        frozen: r.page.as_ref().is_some_and(|p| p.frozen()),
        external: r
            .page
            .as_ref()
            .is_some_and(|p| p.exposure() == crate::basis::cell::page::Exposure::External),
        address: kernel_api::address::VirtAddr::new(r.iova),
        fault_cause: r.info.fault_cause as u64,
        fault_address: r.info.fault_address as u64,
    })
}
pub(crate) fn page(
    actor: &Task,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(FlatRange, Arc<crate::basis::cell::page::Page>), CellFail> {
    let mut records = PROJECTIONS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.generation != expected {
        return Err(CellFail::Busy);
    }
    Ok((
        r.hold.resolve().ok_or(CellFail::Denied)?,
        r.page.clone().ok_or(CellFail::SourceEmpty)?,
    ))
}
/// Producing an owning Slot requires the original private/root authority, not a borrowed Memo.
pub(crate) fn owning_hold(
    actor: &Task,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(Arc<Hold>, Arc<crate::basis::cell::page::Page>), CellFail> {
    let mut records = PROJECTIONS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id || !r.hold.source().manages(actor) {
        return Err(CellFail::NotTransferable);
    }
    if r.generation != expected || r.info.state != MappingState::Settled {
        return Err(CellFail::Busy);
    }
    Ok((r.hold.clone(), r.page.clone().ok_or(CellFail::SourceEmpty)?))
}
pub(crate) fn publish(
    actor: &Arc<Task>,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(), CellFail> {
    let target = target(actor, token, true)?;
    let target_view = target.view().share_view().ok_or(CellFail::Denied)?;
    let _operation = crate::basis::view::share_view::ShareOperation::acquire(&target_view)
        .ok_or(CellFail::Busy)?;
    let mut records = PROJECTIONS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id {
        return Err(CellFail::Denied);
    }
    if r.generation != expected || r.info.state != MappingState::Settled {
        return Err(CellFail::Busy);
    }
    // This platform route is one-shot. Never claim a new hardware publication on a closed domain.
    if r.platform.is_some() {
        return Err(CellFail::NotTransferable);
    }
    let page = r.page.as_ref().ok_or(CellFail::SourceEmpty)?;
    let next = r.generation.checked_add(1).ok_or(CellFail::OoM)?;
    let change = crate::basis::cell::change::acquire(page)?;
    let borrow = ExternalBorrow::prepare(&r.hold, actor, r.modifiable, Some(&change)).map_err(
        |e| match e {
            kernel_api::abi::call::pend::PendFail::Busy => CellFail::Busy,
            kernel_api::abi::call::pend::PendFail::OoM => CellFail::OoM,
            _ => CellFail::Denied,
        },
    )?;
    if !Arc::ptr_eq(borrow.cell(), page) {
        return Err(CellFail::NotTransferable);
    }
    target
        .authority()
        .retain_route()
        .map_err(|_| CellFail::Busy)?;
    let platform = match aperture::bind(&r.source, &actor.ident.team.space.dma) {
        Ok(p) => p,
        Err(e) => {
            unsafe { target.authority().release_route() };
            return Err(failure(e));
        }
    };
    let domain = platform.domain;
    r.platform = Some(platform);
    r.target = Some(target);
    r.borrow = Some(borrow);
    r.generation = next;
    r.freeze = Some(crate::basis::cell::page::Freeze::new(change.clone()));
    r.info.state = MappingState::Publishing;
    let result = retirement::with_space(&actor.ident.team.space.dma, domain, |space| {
        let borrow = r.borrow.as_mut().unwrap();
        let binding =
            unsafe { space.register_during(borrow.cell(), r.iova, r.modifiable, Some(&change)) }?;
        r.binding = Some(binding);
        borrow.publish();
        space.publish(binding)
    });
    if let Err(e) = result {
        close(r);
        return Err(failure(e));
    }
    Ok(())
}
pub(crate) fn retract(
    actor: &Task,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(), CellFail> {
    let mut records = PROJECTIONS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id {
        target(actor, token, true)?;
    }
    if r.generation != expected {
        return Err(CellFail::Busy);
    }
    if !matches!(
        r.info.state,
        MappingState::Active | MappingState::Publishing
    ) {
        return Err(CellFail::Busy);
    }
    let next = r.generation.checked_add(1).ok_or(CellFail::OoM)?;
    r.freeze = Some(crate::basis::cell::page::Freeze::new(
        crate::basis::cell::change::acquire(r.page.as_ref().ok_or(CellFail::SourceEmpty)?)?,
    ));
    r.generation = next;
    close(r);
    Ok(())
}
pub(crate) fn delete(actor: &Task, token: PendId, id: MappingId) -> Result<(), CellFail> {
    let mut records = PROJECTIONS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id {
        target(actor, token, true)?;
    }
    if r.info.state != MappingState::Settled {
        return Err(CellFail::Busy);
    }
    let at = records.iter().position(|r| r.id == id.get()).unwrap();
    records.remove(at);
    Ok(())
}
fn close(record: &mut Projection) {
    if record.info.state == MappingState::Settled {
        return;
    }
    let Some(platform) = &record.platform else {
        return;
    };
    let stopped = aperture::stop(platform.port);
    let closing = retirement::close(platform.domain);
    record.info.state = if stopped.is_ok() && closing.is_ok() {
        MappingState::Retiring
    } else {
        MappingState::Failed
    };
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
        (record.platform.is_some() && record.info.state == MappingState::Settled).then_some(true)
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
            match retirement::fault(record.platform.as_ref().unwrap().domain) {
                Ok(Some(fault)) if fault.device == record.platform.as_ref().unwrap().did => {
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
                record.platform.as_ref().unwrap().domain,
                |space| match space.phase(binding)? {
                    Phase::Publishing => space.ready(binding),
                    Phase::Active => Ok(true),
                    _ => Err(Error::InvalidState),
                },
            );
            match ready {
                Ok(true) => match aperture::activate(record.platform.as_ref().unwrap().port) {
                    Ok(()) => {
                        record.info.state = MappingState::Active;
                        record.freeze = None;
                    }
                    Err(_) => close(record),
                },
                Ok(false) => {}
                Err(_) => close(record),
            }
        }
    }
    if record.platform.as_ref().unwrap().completion.settled() {
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
    if retirement::error(record.platform.as_ref().unwrap().domain).is_ok_and(|e| e.is_some()) {
        record.info.state = MappingState::Failed;
    }
    [None, None]
}
pub(crate) fn maintain() -> bool {
    crate::basis::cell::change::maintain();
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
            let id = record.id;
            let expired = record.info.state == MappingState::Settled
                && record.actor.upgrade().is_none_or(|actor| actor.dead());
            let changed = before != record.info || expired;
            if expired {
                records.remove(at);
            }
            (id, changed, observer)
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
