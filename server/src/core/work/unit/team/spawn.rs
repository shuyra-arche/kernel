use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::abi::call::unit::UnitFail;

use crate::core::memory::address::VirtAddr as KVirt;
use crate::core::memory::nest::NestError;
use crate::core::work::unit::task::Task;

use super::Team;

/// Publish execution after entry/stack preparation. Data access is delegated separately through Pend.
pub(crate) fn spawn(
    target: &Arc<Team>,
    caller: Option<&Arc<Task>>,
    entry: usize,
    words: Vec<usize>,
    stack: usize,
) -> Result<Arc<Task>, UnitFail> {
    fn map_err(error: NestError) -> UnitFail {
        if error == NestError::OutOfMemory {
            UnitFail::OoM
        } else {
            UnitFail::Denied
        }
    }
    let _construction = target.operation().ok_or(UnitFail::Busy)?;
    let first = !target.ready();
    let entry_va = if entry == 0 {
        target.default_entry()
    } else {
        entry
    };
    // 入口的前两个字节都要取得到：取不到就拒收。
    let entry = KVirt::from_raw(entry_va);
    if !entry_va.is_multiple_of(2)
        || target.space.instruction_byte(entry).is_none()
        || target.space.instruction_byte(entry + 1).is_none()
    {
        return Err(UnitFail::BadEntry);
    }
    let mut builder = target.task().entry(KVirt::from_raw(entry_va)).args(words);
    if let Some(caller) = caller {
        builder = builder.controller(caller.ident.id);
    }
    if stack > 0 {
        builder = builder.stack(stack);
    }
    let mut prepared = builder.prepare().map_err(map_err)?;
    prepared
        .publish(|| {
            if first {
                target.set_default_entry(entry_va);
            }
        })
        .map_err(map_err)
}
