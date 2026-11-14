//! One Hart's desired scheduling deadline and its retained programming work.
//! Only the owning Hart calls these methods, with interrupts masked. The
//! comparator's actual/unknown hardware setting remains owned by LocalTimer.
use crate::metal::generic::clock::DISARMED;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) struct Local {
    deadline: AtomicU64,
    pending: AtomicBool,
}
impl Local {
    pub(crate) const fn new() -> Self {
        Self {
            deadline: AtomicU64::new(DISARMED),
            pending: AtomicBool::new(false),
        }
    }
    /// Commit only the software deadline. It must be flushed before Task
    /// return; idle/WFI and boot callers still request immediate refresh.
    pub(crate) fn stage(&self, deadline: u64) {
        self.deadline.store(deadline, Ordering::Relaxed);
        self.pending.store(true, Ordering::Relaxed);
    }
    /// Refresh also covers heap/IPI changes when the local value is unchanged.
    /// Failure retains the work; a later flush must retry the same desired
    /// deadline merged with the then-current shared/retry deadlines.
    pub(crate) fn refresh<E>(&self, program: impl FnOnce(u64) -> Result<(), E>) -> Result<(), E> {
        self.pending.store(true, Ordering::Relaxed);
        program(self.deadline.load(Ordering::Relaxed))?;
        self.pending.store(false, Ordering::Relaxed);
        Ok(())
    }
    pub(crate) fn flush<E>(&self, program: impl FnOnce(u64) -> Result<(), E>) -> Result<bool, E> {
        if !self.pending.load(Ordering::Relaxed) {
            return Ok(false);
        }
        self.refresh(program)?;
        Ok(true)
    }
}
