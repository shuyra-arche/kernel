use crate::basis::nest::Mapping;
use crate::basis::nest::Reservation;
use crate::basis::unit::space::Space;
use alloc::sync::Arc;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use kernel_api::model::identity::TaskId;

use crate::basis::nest::NestError;
use crate::basis::unit::team::Team;
use crate::machine::sync::SpinLock;
use kernel_api::address::PhysAddr;

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

/// 任务自己的内存段：空间、栈（含守卫的保留格）与外壳页。字段由
/// `TaskBuilder::prepare` 按申请—映射—回滚的次序装配。
pub(crate) struct TaskMemory {
    pub(crate) space: Arc<Space>,
    pub(crate) stack: Option<Arc<Mapping>>,
    pub(crate) frame: Option<Arc<Mapping>>,
    pub(crate) guard: Option<Reservation>,
}

impl TaskMemory {
    /// 按段撤下：失败的那一段留在原地，沿用同一身份可重试。
    fn retire(&mut self) -> Result<(), NestError> {
        self.space.nest.retry_invalidation()?;
        if let Some(mapping) = &self.frame {
            if !self.space.nest.settled(mapping.identity()) {
                self.space.withdraw(mapping.whole())?;
            }
            self.frame = None;
        }
        if let Some(mapping) = &self.stack {
            if !self.space.nest.settled(mapping.identity()) {
                self.space.withdraw(mapping.whole())?;
            }
            self.stack = None;
        }
        if let Some(guard) = &self.guard {
            if !self.space.nest.settled(guard.id()) {
                self.space.nest.release_reserved(guard)?;
            }
            self.guard = None;
        }
        self.space.reap_mappings();
        Ok(())
    }
}

impl Drop for TaskMemory {
    fn drop(&mut self) {
        if let Some(mapping) = self.frame.take() {
            let _ = self.space.dispose(mapping);
        }
        if let Some(mapping) = self.stack.take() {
            let _ = self.space.dispose(mapping);
        }
        if let Some(guard) = self.guard.take() {
            let _ = self.space.nest.release_reservation(guard);
        }
    }
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
