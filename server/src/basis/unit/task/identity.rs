use crate::basis::nest::Nest;
use crate::basis::nest::Reservation;
use crate::basis::nest::Span;
use alloc::sync::Arc;

use kernel_api::model::identity::TaskId;

use crate::basis::nest::NestError;
use crate::basis::unit::team::Team;
use crate::metal::sync::SpinLock;
use kernel_api::address::PhysAddr;

static NEXT: crate::basis::name::Serial = crate::basis::name::Serial::new(1, isize::MAX as usize);

/// 下一个任务号：到 `isize::MAX` 就不再发新号（永久耗尽，不回绕）。
pub(crate) fn next_id() -> Result<TaskId, NestError> {
    let id = NEXT.allocate().ok_or(NestError::OutOfMemory)?;
    Ok(TaskId::new(id))
}

/// 任务自己的内存段：空间、栈（含守卫的保留格）与外壳页。字段由
/// `TaskBuilder::prepare` 按申请—映射—回滚的次序装配。
pub(crate) struct TaskMemory {
    pub(crate) nest: Arc<Nest>,
    pub(crate) stack: Option<Arc<Span>>,
    pub(crate) frame: Option<Arc<Span>>,
    pub(crate) guard: Option<Reservation>,
}

impl TaskMemory {
    /// 按段撤下：失败的那一段留在原地，沿用同一身份可重试。
    fn retire(&mut self) -> Result<(), NestError> {
        self.nest.retry_invalidation()?;
        if let Some(mapping) = &self.frame {
            if !self.nest.settled(mapping.identity()) {
                self.nest.detach(mapping.whole())?;
            }
            self.frame = None;
        }
        if let Some(mapping) = &self.stack {
            if !self.nest.settled(mapping.identity()) {
                self.nest.detach(mapping.whole())?;
            }
            self.stack = None;
        }
        if let Some(guard) = &self.guard {
            if !self.nest.settled(guard.id()) {
                self.nest.release_reserved(guard)?;
            }
            self.guard = None;
        }
        self.nest.reap_mappings();
        Ok(())
    }
}

impl Drop for TaskMemory {
    fn drop(&mut self) {
        if let Some(mapping) = self.frame.take() {
            let _ = self.nest.dispose(mapping);
        }
        if let Some(mapping) = self.stack.take() {
            let _ = self.nest.dispose(mapping);
        }
        if let Some(guard) = self.guard.take() {
            let _ = self.nest.release_reservation(guard);
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

    pub(crate) fn receipts(&self) -> crate::basis::unit::team::Clearance {
        crate::basis::unit::team::Clearance::new(self.team.node.clone())
    }
    pub(crate) fn prepare_resident(&self) -> Result<(), NestError> {
        use crate::basis::nest::FaultOutcome;
        use crate::metal::generic::trap::FaultKind;
        let memory = self.memory.lock();
        let (start, size) = memory
            .stack
            .as_ref()
            .ok_or(NestError::NoRegion)?
            .initial_range();
        if size > 64 * 1024 {
            return Err(NestError::NoRegion);
        }
        let pc = unsafe {
            (*(self.frame_pa.as_usize() as *const crate::metal::switcher::context::TrapContext))
                .sepc
        };
        if !matches!(
            self.team.nest.resolve_fault(pc, FaultKind::Instruction)?,
            FaultOutcome::Resolved
        ) {
            return Err(NestError::Busy);
        }
        for offset in (0..size).step_by(kernel_api::address::PAGE_SIZE) {
            if !matches!(
                self.team
                    .nest
                    .resolve_fault(start + offset, FaultKind::Store)?,
                FaultOutcome::Resolved
            ) {
                return Err(NestError::Busy);
            }
        }
        Ok(())
    }

    pub(crate) fn retire(&self) -> Result<(), NestError> {
        self.memory.lock().retire()
    }
}
