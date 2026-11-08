//! Authorized roots for external Nests. All position and hardware responsibility
//! state belongs to the retained Nest; task death only closes admission.
use super::aperture;
use crate::basis::flat::FlatRange;
use crate::basis::nest::PublicationError as Error;
use crate::basis::nest::{Access, Nest, Span};
use crate::basis::pend::{self, ExternalBorrow, Hold, Need};
use crate::basis::sign::{self, WakeKey};
use crate::basis::unit::{anchor::Anchor, task::Task};
use crate::metal::sync::SpinLock;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use kernel_api::abi::call::cell::CellFail;
use kernel_api::model::cell::{CellInfo, CellState, MappingId};
use kernel_api::model::identity::{PendId, TaskId};
use kernel_api::model::modifiable::Modifiable;

struct Registration {
    actor: Weak<Task>,
    task: TaskId,
    source: FlatRange,
    target: Option<Arc<pend::PendEntry>>,
    hold: Arc<Hold>,
    nest: Arc<Nest>,
    span: Arc<Span>,
    requester: Option<u32>,
    modifiable: bool,
    borrow: Option<ExternalBorrow>,
}
impl Registration {
    fn info(&self) -> CellInfo {
        self.span.inspect_memo(0).expect("registered Memo")
    }
    fn idle(&self) -> bool {
        matches!(self.info().state, CellState::Vacant | CellState::Ready)
            && self.nest.external_settled()
    }
}
static NESTS: SpinLock<Vec<Registration>> = SpinLock::new(Vec::new());
// Scheduling count only; authoritative completion is in each retained Nest.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
fn failure(error: Error) -> CellFail {
    match error {
        Error::OutOfMemory | Error::Capacity => CellFail::OutOfMemory,
        Error::Overlap => CellFail::AlreadyMapped,
        Error::InvalidRange => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}
fn target(actor: &Task, token: PendId, write: bool) -> Result<Arc<pend::PendEntry>, CellFail> {
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
    records: &'a mut [Registration],
    actor: &Task,
    token: PendId,
    id: MappingId,
) -> Result<&'a mut Registration, CellFail> {
    let entry = target(actor, token, false)?;
    let range = entry.view().share_view().ok_or(CellFail::Denied)?.source();
    records
        .iter_mut()
        .find(|p| p.span.identity() == id.get() && p.source.same_range(&range))
        .ok_or(CellFail::Denied)
}

pub(crate) fn create(
    actor: &Arc<Task>,
    token: PendId,
    hold: Arc<Hold>,
    iova: usize,
    modifiable: bool,
) -> Result<usize, CellFail> {
    let target = target(actor, token, true)?;
    let pages = hold.resolve().ok_or(CellFail::Denied)?;
    if target.modifiable() != Modifiable::Yes
        || pages.page_count().get() != 1
        || pages.is_borrowed()
    {
        return Err(CellFail::Denied);
    }
    let source = target.view().share_view().ok_or(CellFail::Denied)?.source();
    let access = if modifiable {
        Access::READ | Access::WRITE
    } else {
        Access::READ
    };
    let (nest, span) = Nest::external(iova, pages, access)?;
    let id = span.identity();
    let mut records = NESTS.lock();
    records.try_reserve(1).map_err(|_| CellFail::OutOfMemory)?;
    records.push(Registration {
        actor: Arc::downgrade(actor),
        task: actor.ident.id,
        source,
        target: None,
        hold,
        nest,
        span,
        requester: None,
        modifiable,
        borrow: None,
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
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id
        || !r
            .hold
            .resolve()
            .is_some_and(|range| range.same_range(source))
    {
        return Err(CellFail::Denied);
    }
    if !r.idle() {
        return Err(CellFail::Busy);
    }
    r.nest.associate(&r.span, 0, expected, page)?;
    Ok(())
}
pub(crate) fn snapshot(actor: &Task, token: PendId, id: MappingId) -> Result<CellInfo, CellFail> {
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    let mut info = r.info();
    if r.nest.external_error().is_some() {
        info.state = CellState::Failed;
    }
    Ok(info)
}
pub(crate) fn page(
    actor: &Task,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(FlatRange, Arc<crate::basis::cell::page::Page>), CellFail> {
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.info().generation != expected {
        return Err(CellFail::Busy);
    }
    Ok((
        r.hold.resolve().ok_or(CellFail::Denied)?,
        r.nest.memo_page(&r.span, 0)?.ok_or(CellFail::SourceEmpty)?,
    ))
}
pub(crate) fn owning_hold(
    actor: &Task,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(Arc<Hold>, Arc<crate::basis::cell::page::Page>), CellFail> {
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id || !r.hold.source().manages(actor) {
        return Err(CellFail::NotTransferable);
    }
    if r.info().generation != expected || !r.idle() {
        return Err(CellFail::Busy);
    }
    Ok((
        r.hold.clone(),
        r.nest.memo_page(&r.span, 0)?.ok_or(CellFail::SourceEmpty)?,
    ))
}
pub(crate) fn publish(
    actor: &Arc<Task>,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(), CellFail> {
    let target = target(actor, token, true)?;
    let target_view = target.view().share_view().ok_or(CellFail::Denied)?;
    let _operation =
        crate::basis::view::share::ShareOperation::acquire(&target_view).ok_or(CellFail::Busy)?;
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id {
        return Err(CellFail::Denied);
    }
    if r.info().generation != expected || !r.idle() {
        return Err(CellFail::Busy);
    }
    if r.requester.is_some() {
        return Err(CellFail::NotTransferable);
    }
    let page = r.nest.memo_page(&r.span, 0)?.ok_or(CellFail::SourceEmpty)?;
    let change = crate::basis::cell::change::acquire(&page)?;
    let borrow = ExternalBorrow::prepare(&r.hold, actor, r.modifiable, Some(&change)).map_err(
        |e| match e {
            kernel_api::abi::call::pend::PendFail::Busy => CellFail::Busy,
            kernel_api::abi::call::pend::PendFail::OutOfMemory => CellFail::OutOfMemory,
            _ => CellFail::Denied,
        },
    )?;
    if !Arc::ptr_eq(borrow.cell(), &page) {
        return Err(CellFail::NotTransferable);
    }
    target
        .authority()
        .retain_route()
        .map_err(|_| CellFail::Busy)?;
    let (did, hardware) = match aperture::bind(&r.source, &actor.ident.team.external) {
        Ok(bound) => bound,
        Err(e) => {
            unsafe { target.authority().release_route() };
            return Err(failure(e));
        }
    };
    // A registration binds once; all subsequent failure paths retain the actual Nest.
    if let Err((error, hardware)) = r.nest.bind_external(hardware) {
        // Backend Drop preserves its own unfinished queue/table obligations.
        drop(hardware);
        unsafe { target.authority().release_route() };
        return Err(failure(error));
    }
    r.requester = Some(did);
    r.target = Some(target);
    r.borrow = Some(borrow);
    ACTIVE.fetch_add(1, Ordering::Release);
    if let Err(e) = r
        .nest
        .prepare_external(&r.span, expected, &page, change, r.modifiable)
    {
        close(r);
        return Err(failure(e));
    }
    r.borrow.as_mut().unwrap().publish();
    if let Err(e) = r.nest.submit_external(&r.span) {
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
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id {
        target(actor, token, true)?;
    }
    let info = r.info();
    if info.generation != expected
        || !matches!(info.state, CellState::Published | CellState::Publishing)
    {
        return Err(CellFail::Busy);
    }
    let page = r.nest.memo_page(&r.span, 0)?.ok_or(CellFail::SourceEmpty)?;
    let change = crate::basis::cell::change::acquire(&page)?;
    r.nest.retract_external(&r.span, expected, change)?;
    close(r);
    Ok(())
}
pub(crate) fn delete(actor: &Task, token: PendId, id: MappingId) -> Result<(), CellFail> {
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    if r.task != actor.ident.id {
        target(actor, token, true)?;
    }
    if !r.idle() {
        return Err(CellFail::Busy);
    }
    let at = records
        .iter()
        .position(|r| r.span.identity() == id.get())
        .unwrap();
    records.remove(at);
    Ok(())
}
fn close(record: &Registration) {
    let _ = record.nest.close_external();
}
pub(crate) fn event(
    actor: &Task,
    token: PendId,
    id: MappingId,
    fault: bool,
) -> Result<(Weak<Anchor>, Option<bool>), CellFail> {
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id)?;
    let info = r.info();
    let ready = if info.state == CellState::Failed || r.nest.external_error().is_some() {
        Some(false)
    } else if fault {
        (info.fault_cause != 0).then_some(true)
    } else {
        (r.requester.is_some() && r.idle()).then_some(true)
    };
    Ok((r.nest.life(), ready))
}
fn advance(r: &mut Registration) -> [Option<(TaskId, PendId)>; 2] {
    if r.requester.is_none() || r.idle() && r.borrow.is_none() {
        return [None, None];
    }
    let invalid = r
        .actor
        .upgrade()
        .is_none_or(|a| a.dead() || a.ident.team.external.closed())
        || r.target
            .as_ref()
            .is_none_or(|t| !t.alive() || !t.allows(Need::Write))
        || r.borrow.as_ref().is_some_and(ExternalBorrow::revoked);
    if invalid || r.nest.external_error().is_some() {
        close(r);
    }
    if matches!(r.info().state, CellState::Publishing | CellState::Published) {
        for _ in 0..4 {
            match r.nest.external_fault() {
                Ok(Some(fault)) if Some(fault.device) == r.requester => {
                    r.nest.record_fault(&r.span, fault)
                }
                Ok(None) => break,
                _ => {
                    close(r);
                    break;
                }
            }
        }
    }
    let _ = r.nest.progress_external(&r.span);
    if r.nest.external_settled() {
        let observer = r.borrow.take().and_then(|borrow| {
            ACTIVE.fetch_sub(1, Ordering::AcqRel);
            unsafe { borrow.settle() }
        });
        let route = r.target.take().map(|target| {
            unsafe { target.authority().release_route() };
            (r.task, target.token())
        });
        return [observer, route];
    }
    [None, None]
}
pub(crate) fn maintain() -> bool {
    crate::basis::cell::change::maintain();
    static CURSOR: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..4 {
        let (id, changed, observers) = {
            let _commit = crate::basis::commit();
            let Some(mut records) = NESTS.try_lock() else {
                return true;
            };
            if records.is_empty() {
                break;
            }
            let at = CURSOR.fetch_add(1, Ordering::Relaxed) % records.len();
            let r = &mut records[at];
            let before = r.info();
            let observers = advance(r);
            let id = r.span.identity();
            let expired = r.idle() && r.actor.upgrade().is_none_or(|a| a.dead());
            let changed = before != r.info() || expired;
            if expired {
                records.remove(at);
            }
            (id, changed, observers)
        };
        for (task, token) in observers.into_iter().flatten() {
            pend::notify(task, token);
        }
        if changed {
            sign::signal(WakeKey::ExternalMapping { id });
        }
    }
    if let Some(route) = crate::basis::room::interrupt::external() {
        route.rearm();
    }
    ACTIVE.load(Ordering::Acquire) != 0
}
