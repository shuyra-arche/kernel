//! One monotonic observation window. Samples crossing either boundary remain
//! in aggregate metrics but cannot contaminate a phase-specific distribution.
use core::sync::atomic::{AtomicU64, Ordering};
pub(super) struct Timeline {
    opened: AtomicU64,
    closed: AtomicU64,
}
impl Timeline {
    pub(super) const fn new() -> Self {
        Self {
            opened: AtomicU64::new(0),
            closed: AtomicU64::new(0),
        }
    }
    pub(super) fn open(&self, at: u64) {
        if at != 0 && self.closed.load(Ordering::Acquire) == 0 {
            let _ = self
                .opened
                .compare_exchange(0, at, Ordering::AcqRel, Ordering::Acquire);
        }
    }
    pub(super) fn close(&self, at: u64) {
        if at != 0 {
            let _ = self
                .closed
                .compare_exchange(0, at, Ordering::AcqRel, Ordering::Acquire);
        }
    }
    pub(super) fn bounds(&self) -> (u64, u64) {
        let closed = self.closed.load(Ordering::Acquire);
        (self.opened.load(Ordering::Acquire), closed)
    }
    pub(super) fn phase(&self, begin: u64, end: u64) -> Option<usize> {
        let (opened, closed) = self.bounds();
        if begin > end {
            return None;
        }
        if closed != 0 && begin >= closed {
            return Some(2);
        }
        if opened == 0 || end < opened {
            return (closed == 0 || end < closed).then_some(0);
        }
        if begin >= opened && (closed == 0 || end <= closed) {
            return Some(1);
        }
        None
    }
}
