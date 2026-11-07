use super::page::{Change, Page};
use crate::machine::sync::{Level, SpinLock};
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::abi::call::cell::CellFail;
use kernel_api::model::identity::TaskId;

struct Record {
    owner: TaskId,
    change: Arc<Change>,
}
static LIVE: SpinLock<Vec<Record>> = SpinLock::new_level(Level::L3, Vec::new());

pub(crate) fn begin(owner: TaskId, page: &Arc<Page>) -> Result<usize, CellFail> {
    let mut live = LIVE.lock();
    live.try_reserve(1).map_err(|_| CellFail::OoM)?;
    let change = page
        .begin()
        .map_err(|e| CellFail::from(crate::basis::nest::NestError::from(e)))?;
    let id = change.id();
    live.push(Record { owner, change });
    Ok(id)
}
pub(crate) fn get(owner: TaskId, id: usize) -> Result<Arc<Change>, CellFail> {
    LIVE.lock()
        .iter()
        .find(|r| r.owner == owner && r.change.id() == id)
        .map(|r| r.change.clone())
        .ok_or(CellFail::Denied)
}
pub(crate) fn end(owner: TaskId, id: usize) -> Result<(), CellFail> {
    let removed = {
        let mut live = LIVE.lock();
        let at = live
            .iter()
            .position(|r| r.owner == owner && r.change.id() == id)
            .ok_or(CellFail::Denied)?;
        live.remove(at)
    };
    drop(removed);
    Ok(())
}
pub(crate) fn close(owner: TaskId) {
    loop {
        let removed = {
            let mut live = LIVE.lock();
            live.iter()
                .position(|r| r.owner == owner)
                .map(|at| live.remove(at))
        };
        match removed {
            Some(record) => drop(record),
            None => break,
        }
    }
}

/// Completion notification runs outside Task/Flat/Mapping locks, including death cleanup.
pub(crate) fn maintain() {
    if super::page::change::take_completion() {
        crate::basis::nest::source_mappings::wake_all();
    }
}
