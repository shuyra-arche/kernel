use alloc::sync::Arc;

use crate::basis::unit::task::Task;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::PendId;

pub(crate) fn same(task: &Arc<Task>, a: PendId, b: PendId) -> Result<bool, PendFail> {
    let entries = task.pend.entries.lock();
    let find = |token| {
        entries
            .iter()
            .find(|p| p.token() == token)
            .ok_or(PendFail::Denied)
    };
    let (a, b) = (find(a)?.snapshot(), find(b)?.snapshot());
    if !a.alive() || !b.alive() {
        return Err(PendFail::Dead);
    }
    Ok(a.view().same(b.view()))
}
