//! Find a Page's in-kernel admission gate. This is never an authority table:
//! callers must already hold the source and destination permissions. Memo freeze
//! objects own the gate; the registry holds only Weak references across Task death.
use super::page::{Change, Page};
use crate::machine::sync::{Level, SpinLock};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use kernel_api::abi::call::cell::CellFail;
static LIVE: SpinLock<Vec<Weak<Change>>> = SpinLock::new_level(Level::L3, Vec::new());
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
    live.try_reserve(1).map_err(|_| CellFail::OoM)?;
    let change = page
        .begin()
        .map_err(|e| CellFail::from(crate::basis::nest::NestError::from(e)))?;
    live.push(Arc::downgrade(&change));
    Ok(change)
}
pub(crate) fn find(page: &Arc<Page>) -> Option<Arc<Change>> {
    LIVE.lock()
        .iter()
        .filter_map(Weak::upgrade)
        .find(|c| Arc::ptr_eq(c.page(), page))
}
/// Completion notification runs outside Task/Flat/Mapping locks, including death cleanup.
pub(crate) fn maintain() {
    if super::page::change::take_completion() {
        LIVE.lock().retain(|change| change.strong_count() != 0);
        crate::basis::nest::source_mappings::wake_all();
    }
}
