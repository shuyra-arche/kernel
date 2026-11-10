//! Each Memo retains its creating Pend branch and any published borrow. Task
//! death or branch revocation withdraws that Memo, not the whole External Nest.
use crate::basis::flat::FlatRange;
use crate::basis::nest::PublicationError as Error;
use crate::basis::nest::{Access, Nest, Span};
use crate::basis::pend::{self, ExternalBorrow, Hold};
use crate::basis::sign::{self, WakeKey};
use crate::basis::unit::{anchor::Anchor, task::Task};
use crate::metal::sync::SpinLock;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use kernel_api::abi::call::cell::CellFail;
use kernel_api::model::cell::{CellInfo, CellState, MappingId};
use kernel_api::model::identity::{PendId, TaskId};
use kernel_api::model::pend::{MemoRights, NestRights, Operations};

struct Registration {
    actor: Weak<Task>,
    task: TaskId,
    creator: Arc<pend::PendEntry>,
    target: Option<Arc<pend::PendEntry>>,
    hold: Arc<Hold>,
    nest: Arc<Nest>,
    span: Arc<Span>,
    modifiable: bool,
    borrow: Option<ExternalBorrow>,
}
impl Registration {
    fn info(&self) -> CellInfo {
        self.span.inspect_memo(0).expect("registered Memo")
    }
    fn idle(&self) -> bool {
        matches!(self.info().state, CellState::Vacant | CellState::Ready) && self.borrow.is_none()
    }
    fn orphaned(&self) -> bool {
        !self.creator.alive()
            || self
                .actor
                .upgrade()
                .is_none_or(|actor| actor.dead() || actor.ident.team.external.closed())
    }
    /// The registry removes this entry immediately after successful discharge.
    /// A live publication keeps both its address and its creating branch held.
    fn discard(&self) -> Result<(), CellFail> {
        if !self.idle() {
            return Err(CellFail::Busy);
        }
        self.nest.delete_external_memo(&self.span)?;
        // SAFETY: no publication or borrow remains, and deletion has released
        // this exact Memo's association, freeze and address reservation.
        unsafe { self.creator.authority().release_retirement() };
        Ok(())
    }
}
static NESTS: SpinLock<Vec<Registration>> = SpinLock::new(Vec::new());
fn failure(error: Error) -> CellFail {
    match error {
        Error::OutOfMemory | Error::Capacity => CellFail::OutOfMemory,
        Error::Overlap => CellFail::AlreadyMapped,
        Error::InvalidRange => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}
fn target(
    actor: &Task,
    token: PendId,
    rights: NestRights,
) -> Result<Arc<pend::PendEntry>, CellFail> {
    if let Some(entry) = pend::locate(actor, token) {
        if matches!(entry.target(), pend::Target::Memo(_)) {
            let permission = if rights == NestRights::INSPECT {
                MemoRights::INSPECT
            } else if rights == NestRights::TRANSFER {
                MemoRights::INSERT
            } else if rights == NestRights::PUBLISH {
                MemoRights::PUBLISH
            } else if rights == NestRights::RETRACT {
                MemoRights::RETRACT
            } else if rights == NestRights::RELEASE {
                MemoRights::DELETE
            } else {
                return Err(CellFail::Denied);
            };
            return entry
                .permits(Operations::Memo(permission))
                .then_some(entry)
                .ok_or(CellFail::Denied);
        }
    }
    super::binding::authorized(actor, token, rights).map_err(|_| CellFail::Denied)
}
fn authorized<'a>(
    records: &'a mut [Registration],
    actor: &Task,
    token: PendId,
    id: MappingId,
    rights: NestRights,
) -> Result<&'a mut Registration, CellFail> {
    let entry = target(actor, token, rights)?;
    let nest = match entry.target() {
        pend::Target::Memo(memo) if memo.mapping == id => memo.nest.clone(),
        pend::Target::Memo(_) => return Err(CellFail::Denied),
        _ => entry.target().nest().ok_or(CellFail::Denied)?,
    };
    records
        .iter_mut()
        .find(|p| p.span.identity() == id.get() && Arc::ptr_eq(&p.nest, &nest))
        .ok_or(CellFail::Denied)
}

pub(crate) fn create(
    actor: &Arc<Task>,
    token: PendId,
    hold: Arc<Hold>,
    iova: usize,
    modifiable: bool,
) -> Result<usize, CellFail> {
    let target = target(actor, token, NestRights::RESERVE)?;
    let pages = hold.resolve().ok_or(CellFail::Denied)?;
    if pages.page_count().get() != pages.grain() || pages.is_borrowed() {
        return Err(CellFail::Denied);
    }
    let nest = target.target().nest().ok_or(CellFail::Denied)?;
    if nest.external_phase() != kernel_api::model::nest::NestPhase::Bound
        || (modifiable && target.modifiable() != kernel_api::model::modifiable::Modifiable::Yes)
    {
        return Err(CellFail::Denied);
    }
    let access = if modifiable {
        Access::READ | Access::WRITE
    } else {
        Access::READ
    };
    let mut records = NESTS.lock();
    // The requester can reach every live leaf in this Nest. Another Task
    // must wait for the current Task's positions, including retiring ones.
    if records
        .iter()
        .any(|record| Arc::ptr_eq(&record.nest, &nest) && record.task != actor.ident.id)
    {
        return Err(CellFail::Busy);
    }
    records.try_reserve(1).map_err(|_| CellFail::OutOfMemory)?;
    target
        .authority()
        .retain_retirement()
        .map_err(|_| CellFail::Busy)?;
    let span = match nest.external_memo(iova, pages, access) {
        Ok(span) => span,
        Err(error) => {
            // SAFETY: this attempt has no published or registered Memo.
            unsafe { target.authority().release_retirement() };
            return Err(error.into());
        }
    };
    let id = span.identity();
    records.push(Registration {
        actor: Arc::downgrade(actor),
        task: actor.ident.id,
        creator: target,
        target: None,
        hold,
        nest,
        span,
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
    block_owner: Option<&pend::Pend>,
) -> Result<(), CellFail> {
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id, NestRights::TRANSFER)?;
    if r.orphaned()
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
    let (offset, count) = r.hold.window();
    r.hold
        .source()
        .check_move(offset, count, block_owner)
        .map_err(|_| CellFail::Busy)?;
    r.hold
        .source()
        .retain_blocked(offset, count, &page)
        .map_err(|_| CellFail::Busy)?;
    r.nest.associate(&r.span, 0, expected, page)?;
    Ok(())
}
pub(crate) fn remove(
    actor: &Task,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(), CellFail> {
    let mut records = NESTS.lock();
    let entry = pend::locate(actor, token)
        .filter(|e| e.permits(Operations::Memo(MemoRights::REMOVE)))
        .ok_or(CellFail::Denied)?;
    let pend::Target::Memo(memo) = entry.target() else {
        return Err(CellFail::Denied);
    };
    let r = records
        .iter_mut()
        .find(|r| {
            memo.mapping == id && r.span.identity() == id.get() && Arc::ptr_eq(&r.nest, &memo.nest)
        })
        .ok_or(CellFail::Denied)?;
    if r.orphaned() {
        return Err(CellFail::Denied);
    }
    if !r.idle() {
        return Err(CellFail::Busy);
    }
    r.nest.remove_memo(&r.span, 0, expected)?;
    Ok(())
}
pub(crate) fn snapshot(actor: &Task, token: PendId, id: MappingId) -> Result<CellInfo, CellFail> {
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id, NestRights::INSPECT)?;
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
    let r = authorized(&mut records, actor, token, id, NestRights::TRANSFER)?;
    if r.info().generation != expected {
        return Err(CellFail::Busy);
    }
    Ok((
        r.hold.resolve().ok_or(CellFail::Denied)?,
        r.nest.memo_page(&r.span, 0)?.ok_or(CellFail::SourceEmpty)?,
    ))
}
pub(crate) fn publish(
    actor: &Arc<Task>,
    token: PendId,
    id: MappingId,
    expected: u64,
) -> Result<(), CellFail> {
    let target = target(actor, token, NestRights::PUBLISH)?;
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id, NestRights::PUBLISH)?;
    if r.orphaned() {
        return Err(CellFail::Denied);
    }
    if r.info().generation != expected || !r.idle() {
        return Err(CellFail::Busy);
    }
    if r.nest.external_phase() != kernel_api::model::nest::NestPhase::Bound {
        return Err(CellFail::Denied);
    }
    if r.modifiable && target.modifiable() != kernel_api::model::modifiable::Modifiable::Yes {
        return Err(CellFail::Denied);
    }
    let page = r.nest.memo_page(&r.span, 0)?.ok_or(CellFail::SourceEmpty)?;
    let change = crate::basis::cell::change::acquire(&page)?;
    let creator = r.actor.upgrade().ok_or(CellFail::Denied)?;
    // Delegation changes the Memo actor, not the source ledger owner. Never
    // wait for a second ledger under COMMIT/NESTS; contention is retryable.
    let _creator_serial = if Arc::ptr_eq(&creator, actor) {
        None
    } else {
        Some(creator.pend.try_lock().ok_or(CellFail::Busy)?)
    };
    let borrow = ExternalBorrow::prepare(&r.hold, &creator, r.modifiable, Some(&change)).map_err(
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
    r.target = Some(target);
    r.borrow = Some(borrow);
    if let Err(e) = r
        .nest
        .prepare_external(&r.span, expected, &page, change, r.modifiable)
    {
        r.borrow.take();
        if let Some(target) = r.target.take() {
            unsafe { target.authority().release_route() };
        }
        return Err(failure(e));
    }
    r.borrow.as_mut().unwrap().publish();
    if let Err(e) = r.nest.submit_external(&r.span) {
        // A partial hardware failure may poison the entire context. Unlike a
        // branch return, that failure deliberately closes the whole Nest.
        let _ = r.nest.close_external();
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
    let r = authorized(&mut records, actor, token, id, NestRights::RETRACT)?;
    let info = r.info();
    if info.generation != expected
        || !matches!(info.state, CellState::Published | CellState::Publishing)
    {
        return Err(CellFail::Busy);
    }
    let page = r.nest.memo_page(&r.span, 0)?.ok_or(CellFail::SourceEmpty)?;
    let change = crate::basis::cell::change::acquire(&page)?;
    r.nest.retract_external(&r.span, expected, change)?;
    Ok(())
}
pub(crate) fn delete(actor: &Task, token: PendId, id: MappingId) -> Result<(), CellFail> {
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id, NestRights::RELEASE)?;

    r.discard()?;
    let at = records
        .iter()
        .position(|r| r.span.identity() == id.get())
        .unwrap();
    records.remove(at);
    Ok(())
}
pub(crate) fn event(
    actor: &Task,
    token: PendId,
    id: MappingId,
    published: bool,
    expected: u64,
) -> Result<
    (
        Weak<Anchor>,
        Option<Option<kernel_api::abi::call::sign::SignEventFail>>,
    ),
    CellFail,
> {
    use kernel_api::abi::call::sign::SignEventFail;
    let mut records = NESTS.lock();
    let r = authorized(&mut records, actor, token, id, NestRights::INSPECT)?;
    let info = r.info();
    if expected == 0 {
        return Err(CellFail::Denied);
    }
    let ready = if info.generation > expected {
        Some(Some(SignEventFail::Gone))
    } else if info.generation < expected {
        let next = info.generation.checked_add(1) == Some(expected)
            && if published {
                info.state == CellState::Ready
            } else {
                matches!(info.state, CellState::Published | CellState::Publishing)
            };
        if next {
            None
        } else {
            Some(Some(SignEventFail::Gone))
        }
    } else if info.state == CellState::Failed || r.nest.external_error().is_some() {
        Some(Some(SignEventFail::Busy))
    } else if if published {
        info.state == CellState::Published
    } else {
        r.idle()
    } {
        Some(None)
    } else {
        None
    };
    Ok((r.nest.life(), ready))
}
fn advance(r: &mut Registration) -> [Option<(TaskId, PendId)>; 2] {
    if r.borrow.is_none() {
        return [None, None];
    }
    let invalid = r.orphaned()
        || r.target.as_ref().is_none_or(|t| {
            !t.permits(Operations::Memo(MemoRights::PUBLISH))
                && !t.permits(Operations::Nest(NestRights::PUBLISH))
        })
        || r.borrow.as_ref().is_some_and(ExternalBorrow::revoked);
    if invalid && matches!(r.info().state, CellState::Publishing | CellState::Published) {
        if let Ok(Some(page)) = r.nest.memo_page(&r.span, 0) {
            if let Ok(change) = crate::basis::cell::change::acquire(&page) {
                let _ = r
                    .nest
                    .retract_external(&r.span, r.info().generation, change);
            }
        }
    }
    let _ = r.nest.progress_external(&r.span);
    if matches!(r.info().state, CellState::Ready | CellState::Vacant) {
        let observer = r
            .borrow
            .take()
            .and_then(|borrow| unsafe { borrow.settle() });
        let route = r.target.take().map(|target| {
            unsafe { target.authority().release_route() };
            (r.task, target.token())
        });
        [observer, route]
    } else {
        [None, None]
    }
}
pub(crate) fn maintain() -> bool {
    crate::basis::cell::change::maintain();
    static CURSOR: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..4 {
        let (id, changed, observers, discarded) = {
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
            let changed = before != r.info();
            // A branch return retains its creating registration until the
            // exact Memo is idle and removed. No unrelated leaf is discarded.
            let remove = r.orphaned() && r.discard().is_ok();
            let discarded = remove.then_some((r.task, r.creator.token()));
            if remove {
                records.remove(at);
            }
            (id, changed || remove, observers, discarded)
        };
        for (task, token) in observers.into_iter().flatten().chain(discarded) {
            pend::notify(task, token);
        }
        if changed {
            sign::signal(WakeKey::ExternalMapping { id });
        }
    }
    if let Some(route) = crate::basis::room::interrupt::external() {
        route.rearm();
    }
    super::binding::maintain()
}

/// Whole-Nest deletion discards ready positions only after binding settlement.
/// Published/retiring positions retain their source and address for later progress.
pub(super) fn discard(nest: &Arc<Nest>) -> bool {
    let mut records = NESTS.lock();
    if records
        .iter()
        .any(|r| Arc::ptr_eq(&r.nest, nest) && !r.idle())
    {
        return false;
    }
    let mut at = 0;
    while at < records.len() {
        if Arc::ptr_eq(&records[at].nest, nest) {
            if records[at].discard().is_err() {
                return false;
            }
            records.remove(at);
        } else {
            at += 1;
        }
    }
    true
}

/// Called after logical revocation and outside ledger/COMMIT locks. Published
/// branches keep their route hold until maintain observes exact completion.
pub(crate) fn withdraw_memo(
    memo: &super::memo::Memo,
    entry: &pend::PendEntry,
) -> Result<(), kernel_api::abi::call::pend::PendFail> {
    use kernel_api::abi::call::pend::PendFail;
    let _commit = crate::basis::commit();
    let mut records = NESTS.lock();
    let Some(at) = records
        .iter()
        .position(|r| r.span.identity() == memo.mapping.get() && Arc::ptr_eq(&r.nest, &memo.nest))
    else {
        return Ok(());
    };
    let r = &mut records[at];
    let root = entry.sire().is_none() && entry.authority().retiring();
    if !root
        && !r
            .target
            .as_ref()
            .is_some_and(|target| Arc::ptr_eq(target.authority(), entry.authority()))
    {
        return Ok(());
    }
    if !r.idle() {
        if matches!(r.info().state, CellState::Published | CellState::Publishing) {
            let page = r
                .nest
                .memo_page(&r.span, 0)
                .map_err(|_| PendFail::Busy)?
                .ok_or(PendFail::Busy)?;
            let change = crate::basis::cell::change::acquire(&page).map_err(|_| PendFail::Busy)?;
            r.nest
                .retract_external(&r.span, r.info().generation, change)
                .map_err(|_| PendFail::Busy)?;
        }
        return Err(PendFail::Busy);
    }
    if entry.sire().is_none() && entry.authority().retiring() {
        r.discard().map_err(|_| PendFail::Busy)?;
        records.remove(at);
    }
    Ok(())
}

pub(crate) fn block_source(
    source: &Arc<pend::SourceAuthorization>,
    selected: &FlatRange,
    block: &pend::RangeBlock,
) -> Result<(), kernel_api::abi::call::pend::PendFail> {
    use kernel_api::abi::call::pend::PendFail;
    let mut records = NESTS.lock();
    let mut busy = false;
    for r in records
        .iter_mut()
        .filter(|r| Arc::ptr_eq(r.hold.source(), source))
    {
        let (offset, count) = r.hold.window();
        let base = source.full_range().ok_or(PendFail::Denied)?.index().get();
        if base + offset.get() >= selected.index().get() + selected.page_count().get()
            || base + offset.get() + count.get() <= selected.index().get()
        {
            continue;
        }
        if let Some(page) = r.nest.memo_page(&r.span, 0).map_err(|_| PendFail::Busy)? {
            block.retain(page.clone())?;
            if matches!(r.info().state, CellState::Published | CellState::Publishing) {
                let change =
                    crate::basis::cell::change::acquire(&page).map_err(|_| PendFail::Busy)?;
                r.nest
                    .retract_external(&r.span, r.info().generation, change)
                    .map_err(|_| PendFail::Busy)?;
            }
        }
        busy |= !r.idle();
    }
    if busy { Err(PendFail::Busy) } else { Ok(()) }
}
