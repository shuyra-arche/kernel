use crate::basis::nest::Nest;
use crate::basis::nest::Span;
use alloc::sync::Arc;

use kernel_api::model::identity::TaskId;

use crate::basis::nest::NestError;
use crate::basis::sync::LegacyLock;
use crate::basis::unit::team::Team;
use kernel_api::address::PhysAddr;

static NEXT: crate::basis::name::Serial = crate::basis::name::Serial::new(1, isize::MAX as usize);

/// 下一个任务号：到 `isize::MAX` 就不再发新号（永久耗尽，不回绕）。
pub(crate) fn next_id() -> Result<TaskId, NestError> {
    let id = NEXT.allocate().ok_or(NestError::OutOfMemory)?;
    Ok(TaskId::new(id))
}

/// Kernel-owned execution storage. Ordinary User Tasks retain no stack owner;
/// bootstrap transitions alone carry an explicitly private stack receipt.
pub(crate) struct TaskMemory {
    pub(crate) nest: Arc<Nest>,
    pub(crate) bootstrap: Option<super::creation::BootstrapStack>,
    pub(crate) start: kernel_api::model::unit::TaskStart,
    pub(crate) frame: Option<Arc<Span>>,
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
        if let Some(stack) = &mut self.bootstrap {
            stack.retire()?;
            self.bootstrap = None;
        }
        self.nest.reap_reservations();
        Ok(())
    }
}

impl Drop for TaskMemory {
    fn drop(&mut self) {
        if let Some(mapping) = self.frame.take() {
            let _ = self.nest.dispose(mapping);
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
    memory: LegacyLock<crate::basis::sync::order::TaskMemory, Option<TaskMemory>>,
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
            memory: LegacyLock::new(Some(memory)),
        }
    }

    pub(crate) fn receipts(&self) -> crate::basis::unit::team::Clearance {
        crate::basis::unit::team::Clearance::new(self.team.node.clone())
    }
    pub(crate) fn prepare_resident(&self) -> Result<(), NestError> {
        use crate::basis::nest::FaultOutcome;
        use crate::metal::generic::trap::FaultKind;
        // Scheduler admission owns its existing Preparation lease until this
        // method returns. Task::retire observes preparing() and cannot take the
        // original TaskMemory while these immutable coordinates are resolved.
        let (start, size) = {
            let stored = self.memory.lock();
            let memory = stored.as_ref().ok_or(NestError::Busy)?;
            memory.frame.as_ref().ok_or(NestError::NoRegion)?;
            if let Some(stack) = &memory.bootstrap {
                stack
                    .mapping
                    .as_ref()
                    .ok_or(NestError::NoRegion)?
                    .initial_range()
            } else {
                self.team
                    .nest
                    .mapping_range(memory.start.stack_pointer - 1)
                    .ok_or(NestError::NoRegion)?
            }
        };
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
        let memory = {
            self.memory
                .try_lock()
                .ok_or(NestError::Busy)?
                .take()
                .ok_or(NestError::Busy)?
        };
        let mut progress = MemoryRetirement {
            owner: self,
            memory: Some(memory),
        };
        progress
            .memory
            .as_mut()
            .expect("owned memory continuation")
            .retire()
    }
}

// None in storage means the original TaskMemory is owned by this synchronous
// retirement lease. Busy/partial failure restores it with completed stages
// retained; no duplicate Page/Span ownership or new resource authority exists.
struct MemoryRetirement<'a> {
    owner: &'a TaskIdent,
    memory: Option<TaskMemory>,
}
impl Drop for MemoryRetirement<'_> {
    fn drop(&mut self) {
        let mut stored = self.owner.memory.lock();
        assert!(stored.is_none());
        *stored = self.memory.take();
    }
}
