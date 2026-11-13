use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use kernel_api::abi::call::cell::{CellCall, CellFail};
use kernel_api::address::{PageCount, PageOffset, VirtAddr as KVirt};
use kernel_api::model::{identity::PendId, modifiable::Modifiable};

use crate::basis::cell::transfer::{Transfer, TransferError};
use crate::basis::flat::{Flat, FlatError, FlatRange};
use crate::basis::nest::{Access, Nest, NestError, Span};
use crate::basis::pend::{self, Geometry, Hold, SourceAuthorization};
use crate::basis::sign::{self, WakeKey};
use crate::basis::unit::task::{Task, TaskIdent};
use crate::metal::switcher::context::TrapContext;

mod object;

pub(super) fn dispatch(frame: &mut TrapContext, call: CellCall, ident: &Arc<TaskIdent>) {
    super::outcome::answer(frame, object::execute(call, ident));
}

fn read_snapshot<const N: usize>(space: &Nest, at: KVirt) -> Result<[u8; N], CellFail> {
    let mut bytes = [0; N];
    if !space.copy_in(&mut bytes, at) {
        return Err(CellFail::Denied);
    }
    Ok(bytes)
}

fn caller(ident: &Arc<TaskIdent>) -> Option<Arc<Task>> {
    use crate::basis::room::scheduler::slot::current;
    use crate::basis::unit::task::registry::find;
    current()
        .running_task()
        .or_else(|| find(ident.id).and_then(|task| task.upgrade()))
}

fn source_fail(error: kernel_api::abi::call::pend::PendFail) -> CellFail {
    use kernel_api::abi::call::pend::PendFail;
    match error {
        PendFail::OutOfMemory => CellFail::OutOfMemory,
        PendFail::Busy => CellFail::Busy,
        PendFail::NotAligned => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}

/// Legacy v10 creation returns a SlotId; Pend-backed association does not.
/// Caller holds the Task ledger serial and COMMIT until the reference is stored.
fn create(
    task: &Arc<Task>,
    license: PendId,
    allocation: kernel_api::model::cell::Allocation,
) -> Result<usize, CellFail> {
    let (range, authorization) =
        crate::basis::flat::authority::allocate(task, license, allocation).map_err(source_fail)?;
    let ceiling = authorization.ceiling();
    task.pend
        .references
        .insert(
            authorization,
            PageOffset::ZERO,
            range.page_count(),
            ceiling,
            None,
        )
        .map_err(source_fail)
}

fn release(
    reference: kernel_api::model::cell::SlotId,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let task = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = task.pend.lock();
    if !pend::live(&task) {
        return Err(CellFail::Denied);
    }
    let hold = task
        .pend
        .references
        .remove(reference.get())
        .ok_or(CellFail::Denied)?;
    drop(hold);
    Ok(0)
}

fn next_flight_key() -> Result<usize, CellFail> {
    const FLIGHT_DOMAIN: usize = 1 << (usize::BITS - 1);
    static NEXT: crate::basis::name::Serial =
        crate::basis::name::Serial::new(1, isize::MAX as usize);
    NEXT.allocate()
        .map(|id| FLIGHT_DOMAIN | id)
        .ok_or(CellFail::OutOfMemory)
}

fn transfer(
    from: kernel_api::model::cell::SlotId,
    from_page: usize,
    to: kernel_api::model::cell::SlotId,
    to_page: usize,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let caller = caller(ident).ok_or(CellFail::Denied)?;
    let serial = caller.pend.lock();
    if !pend::live(&caller) {
        return Err(CellFail::Denied);
    }
    let source = endpoint(&caller, from)?;
    let target = endpoint(&caller, to)?;
    let transfer = Transfer::new(
        source,
        PageOffset::new(from_page),
        target,
        PageOffset::new(to_page),
    )
    .map_err(CellFail::from)?;
    let source_page = endpoint(&caller, from)?
        .page(PageOffset::new(from_page))?
        .ok_or(CellFail::SourceEmpty)?;
    let permit = crate::basis::cell::change::find(&source_page);
    let flight = next_flight_key()?;
    let mut ends: Vec<(Arc<Hold>, Arc<Geometry>)> = Vec::new();
    ends.try_reserve_exact(2)
        .map_err(|_| CellFail::OutOfMemory)?;
    for (reference, page) in [(from, from_page), (to, to_page)] {
        let hold = caller
            .pend
            .references
            .get(reference.get())
            .ok_or(CellFail::Denied)?;
        let offset = PageOffset::new(
            hold.window()
                .0
                .get()
                .checked_add(page)
                .ok_or(CellFail::NotAligned)?,
        );
        let count = PageCount::new(source_page.pages()).ok_or(CellFail::NotAligned)?;
        let window = Geometry::new(offset, count).map_err(source_fail)?;
        ends.push((hold, window));
    }
    {
        let _commit = crate::basis::commit();
        for (hold, window) in &ends {
            let (offset, count) = window.window();
            if !hold.source().branch_check(
                hold.authority().map(Arc::as_ref),
                Access::READ | Access::WRITE,
            ) {
                return Err(CellFail::Denied);
            }
            // Recheck both complete endpoints at the admission commit, including
            // TRANSFER after preparation; ASSOCIATE alone is not movement authority.
            if let Some((token, authority)) = hold.branch() {
                use kernel_api::model::pend::{FlatRights, Operations, PendRange, PendScope};
                let owns = match authority.limits().operations {
                    Operations::Flat(_) => authority.selects(
                        Operations::Flat(FlatRights::TRANSFER),
                        PendScope::Pages(PendRange::Pages {
                            offset: offset.get(),
                            count: count.get(),
                        }),
                    ),
                    Operations::Share(_) => hold
                        .source()
                        .root()
                        .is_some_and(|(_, root, _)| token == root),
                    _ => false,
                };
                if !owns {
                    return Err(CellFail::Denied);
                }
            }
            if !hold
                .resolve_window(offset, count)
                .is_some_and(|range| range.level() == source_page.level())
            {
                return Err(CellFail::Denied);
            }
            hold.source()
                .check_move(offset, count, hold.authority().map(Arc::as_ref))
                .map_err(source_fail)?;
        }
        for (hold, window) in &ends {
            let (offset, count) = window.window();
            hold.source()
                .retain_blocked(offset, count, &source_page)
                .map_err(source_fail)?;
        }
        for (at, (hold, window)) in ends.iter().enumerate() {
            if let Err(error) = hold.source().enter_publish_range(
                flight,
                window.clone(),
                hold.authority().map(Arc::as_ref),
            ) {
                for (held, _) in &ends[..at] {
                    held.source().leave_publish(flight);
                }
                return Err(source_fail(error));
            }
        }
    }
    let outcome = transfer.execute(permit.as_ref());
    for (hold, _) in &ends {
        hold.source().leave_publish(flight);
    }
    drop(serial);
    wake(outcome.affected);
    outcome.result.map(|()| 0).map_err(CellFail::from)
}

fn endpoint(
    caller: &Arc<Task>,
    reference: kernel_api::model::cell::SlotId,
) -> Result<FlatRange, CellFail> {
    let hold = caller
        .pend
        .references
        .get(reference.get())
        .ok_or(CellFail::Denied)?;
    let flat = hold.authority().is_some_and(|p| {
        matches!(
            p.limits().operations,
            kernel_api::model::pend::Operations::Flat(_)
        )
    });
    if !flat && !hold.source().manages(caller) {
        return Err(CellFail::Denied);
    }
    hold.resolve().ok_or(CellFail::Denied)
}

fn wake(affected: Vec<Weak<Span>>) {
    for relation in affected {
        let Some(mapping) = relation.upgrade() else {
            continue;
        };
        let key = mapping.wake_key();
        drop(mapping);
        if let Some((space, mapping)) = key {
            sign::signal(WakeKey::Mapping { space, mapping });
        }
    }
}

impl From<NestError> for CellFail {
    fn from(error: NestError) -> Self {
        match error {
            NestError::OutOfMemory => CellFail::OutOfMemory,
            NestError::NotAligned => CellFail::NotAligned,
            NestError::NoRegion | NestError::Missing => CellFail::NoRegion,
            NestError::AlreadyMapped => CellFail::AlreadyMapped,
            NestError::WidenDenied => CellFail::WidenDenied,
            NestError::Shootdown | NestError::Busy => CellFail::Busy,
            NestError::Pinned | NestError::NotMapped | NestError::DramOverlap => CellFail::Denied,
        }
    }
}

impl From<FlatError> for CellFail {
    fn from(error: FlatError) -> Self {
        NestError::from(error).into()
    }
}

impl From<TransferError> for CellFail {
    fn from(error: TransferError) -> Self {
        match error {
            TransferError::OutOfMemory => CellFail::OutOfMemory,
            TransferError::OutOfRange => CellFail::NoRegion,
            TransferError::SourceEmpty => CellFail::SourceEmpty,
            TransferError::DestinationOccupied => CellFail::DestinationOccupied,
            TransferError::NotTransferable => CellFail::NotTransferable,
            TransferError::Busy => CellFail::Busy,
        }
    }
}
