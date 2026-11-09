//! FIFO exclusion admission. A Hart retains its turn until the owner leaves;
//! failed nonblocking admission never reserves or abandons a turn.
use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub(super) struct Turns {
    next: AtomicUsize,
    serving: AtomicUsize,
}
impl Turns {
    pub(super) const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            serving: AtomicUsize::new(0),
        }
    }
    pub(super) fn enter(&self) {
        let ticket = self.next.fetch_add(1, Ordering::Relaxed);
        while self.serving.load(Ordering::Acquire) != ticket {
            crate::metal::stop::hush();
            crate::metal::interrupt::window::poll();
            core::hint::spin_loop();
        }
    }
    pub(super) fn try_enter(&self) -> bool {
        // Acquire observes the prior owner's protected writes. Reserve only
        // an empty queue, including the gap between two already queued owners.
        let serving = self.serving.load(Ordering::Acquire);
        self.next
            .compare_exchange(
                serving,
                serving.wrapping_add(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
    }
    pub(super) fn leave(&self) {
        self.serving.fetch_add(1, Ordering::Release);
    }
}
