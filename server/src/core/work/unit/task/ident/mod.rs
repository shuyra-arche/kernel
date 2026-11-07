use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};

use kernel_api::model::identity::TaskId;

use crate::core::memory::address::PhysAddr;
use crate::core::memory::nest::NestError;
use crate::core::work::unit::team::Team;
use crate::machine::sync::SpinLock;

pub(crate) use memory::TaskMemory;

mod memory;

static NEXT: AtomicUsize = AtomicUsize::new(1);

/// 下一个任务号：到 `isize::MAX` 就不再发新号（既有回绕口径）。
pub(crate) fn next_id() -> Result<TaskId, NestError> {
    let id = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            (id < isize::MAX as usize).then_some(id + 1)
        })
        .map_err(|_| NestError::OutOfMemory)?;
    Ok(TaskId::new(id))
}

/// 一个任务的身份：号、所属团队、外壳页的物理（调试下还有虚拟）地址，以及它自己
/// 的那段内存。
pub(crate) struct TaskIdent {
    pub(crate) id: TaskId,
    pub(crate) controller: TaskId,
    pub(crate) team: Arc<Team>,
    pub(crate) frame_pa: PhysAddr,
    memory: SpinLock<TaskMemory>,
}

impl TaskIdent {
    pub(crate) fn new(
        id: TaskId,
        controller: TaskId,
        team: Arc<Team>,
        frame_pa: PhysAddr,
        memory: TaskMemory,
    ) -> TaskIdent {
        TaskIdent {
            id,
            controller,
            team,
            frame_pa,
            memory: SpinLock::new(memory),
        }
    }

    pub(crate) fn retire(&self) -> Result<(), NestError> {
        self.memory.lock().retire()
    }
}
