//! Retained local retry deadline. Only a due timer observation consumes it;
//! ordinary comparator refreshes and earlier timers cannot lose deferred work.
use crate::metal::generic::clock::DISARMED;
use core::sync::atomic::{AtomicU64, Ordering};

pub(super) struct Retry {
    at: AtomicU64,
}
impl Retry {
    pub(super) const fn new() -> Self {
        Self {
            at: AtomicU64::new(DISARMED),
        }
    }
    /// Owner-Hart only, with interrupts masked. Keep an earlier future retry;
    /// replace an elapsed retry so contention cannot cause an immediate storm.
    pub(super) fn request(&self, now: u64, delay: u64) -> u64 {
        let next = now.saturating_add(delay.max(1)).min(DISARMED - 1);
        let pending = self.deadline();
        let at = if pending > now {
            pending.min(next)
        } else {
            next
        };
        self.at.store(at, Ordering::Relaxed);
        at
    }
    pub(super) fn deadline(&self) -> u64 {
        self.at.load(Ordering::Relaxed)
    }
    pub(super) fn elapsed(&self, now: u64) {
        if self.deadline() <= now {
            self.at.store(DISARMED, Ordering::Relaxed);
        }
    }
}
