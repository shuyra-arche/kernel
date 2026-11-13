//! Find a Page's in-kernel admission gate. This is never an authority table:
//! callers must already hold the source and destination permissions. Memo freeze
//! objects own the gate; the registry holds only Weak references across Task death.
use super::page::{Change, Page};
use crate::metal::sync::{Level, SpinLock};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use kernel_api::abi::call::cell::CellFail;
static LIVE: SpinLock<Vec<Weak<Change>>> = SpinLock::new_level(Level::L3, Vec::new());

/// Caller holds COMMIT and the holding's Flat lock. Detach the registry's weak
/// upgrade path while proving exclusive ownership; restore it without allocation
/// on refusal. No hardware receipt is discharged by this operation.
pub(crate) fn release_block(
    mut block: super::page::Block,
    page: &mut Arc<Page>,
) -> Result<(), super::page::Block> {
    let mut live = LIVE.lock();
    let at = live
        .iter()
        .position(|known| known.as_ptr() == Arc::as_ptr(block.change()));
    if let Some(at) = at {
        live.remove(at);
    }
    if block.release_unique(page) {
        drop(block);
        Ok(())
    } else {
        if let Some(at) = at {
            live.insert(at, Arc::downgrade(block.change()));
        }
        Err(block)
    }
}
pub(crate) fn acquire(page: &Arc<Page>) -> Result<Arc<Change>, CellFail> {
    let mut live = LIVE.lock();
    live.retain(|change| change.strong_count() != 0);
    if let Some(change) = live
        .iter()
        .filter_map(Weak::upgrade)
        .find(|c| Arc::ptr_eq(c.page(), page))
    {
        return Ok(change);
    }
    live.try_reserve(1).map_err(|_| CellFail::OutOfMemory)?;
    let change = page.begin().map_err(|error| match error {
        super::page::RetainError::Exhausted => CellFail::OutOfMemory,
        super::page::RetainError::Quarantined => CellFail::Denied,
        super::page::RetainError::Busy | super::page::RetainError::Borrowed => CellFail::Busy,
        super::page::RetainError::Unbalanced => unreachable!("unbalanced Page change"),
    })?;
    live.push(Arc::downgrade(&change));
    Ok(change)
}
pub(crate) fn find(page: &Arc<Page>) -> Option<Arc<Change>> {
    LIVE.lock()
        .iter()
        .filter_map(Weak::upgrade)
        .find(|c| Arc::ptr_eq(c.page(), page))
}
/// Completion notification runs outside Task/Flat/Reservation locks, including death cleanup.
pub(crate) fn maintain() {
    if super::page::reclaim::change::take_completion() {
        LIVE.lock().retain(|change| change.strong_count() != 0);
        crate::basis::nest::index::source::wake_all();
    }
}
