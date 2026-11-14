//! Caller lookup and admission error translation.
use crate::basis::unit::task::{Task, TaskIdent};
use alloc::sync::Arc;
pub(super) fn caller(ident: &Arc<TaskIdent>) -> Option<Arc<Task>> {
    use crate::basis::room::scheduler::slot::current;
    use crate::basis::unit::task::registry::find;
    current()
        .running_task()
        .or_else(|| find(ident.id).and_then(|task| task.upgrade()))
}

pub(super) fn source_fail(
    error: kernel_api::abi::call::pend::PendFail,
) -> crate::basis::nest::memo::AccessError {
    use crate::basis::nest::memo::AccessError;
    use kernel_api::abi::call::pend::PendFail;
    match error {
        PendFail::OutOfMemory => AccessError::OutOfMemory,
        PendFail::Busy => AccessError::Busy,
        PendFail::NotAligned => AccessError::NotAligned,
        _ => AccessError::Denied,
    }
}
