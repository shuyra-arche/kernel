use alloc::sync::Arc;

use crate::core::memory::nest::{Mapping, NestError, Reservation};
use crate::core::work::unit::space::Space;

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
    pub(super) fn retire(&mut self) -> Result<(), NestError> {
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
