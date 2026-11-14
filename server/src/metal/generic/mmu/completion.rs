//! Coalescing all-CPU completion rounds. One sequence orders every initiator's
//! preceding edits; a later round therefore covers earlier rounds too.
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Round(u64);

pub(crate) struct Sequence(AtomicU64);
impl Sequence {
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }
    /// AcqRel chains edits from concurrent initiators. Never wrap: a delayed
    /// receipt must not become evidence for a different boot-lifetime round.
    pub(crate) fn next(&self) -> Option<Round> {
        self.0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .ok()
            .map(|n| Round(n + 1))
    }
}

pub(crate) struct Mailbox {
    requested: AtomicU64,
    completed: AtomicU64,
}
impl Mailbox {
    pub(crate) const fn new() -> Self {
        Self {
            requested: AtomicU64::new(0),
            completed: AtomicU64::new(0),
        }
    }
    /// Every mailbox must use the same Sequence and the same global operation.
    pub(crate) fn request(&self, round: Round) {
        self.requested.fetch_max(round.0, Ordering::Release);
    }
    pub(crate) fn pending(&self) -> Option<Round> {
        let requested = self.requested.load(Ordering::Acquire);
        (requested > self.completed.load(Ordering::Relaxed)).then_some(Round(requested))
    }
    /// # Safety
    /// On this mailbox's CPU, after acquiring this exact pending round, execute
    /// global translation invalidation, prior explicit-access ordering and
    /// instruction synchronization. An IPI or SFENCE alone is insufficient.
    pub(crate) unsafe fn complete(&self, round: Round) {
        self.completed.fetch_max(round.0, Ordering::Release);
    }
    pub(crate) fn covers(&self, round: Round) -> bool {
        self.completed.load(Ordering::Acquire) >= round.0
    }
}
