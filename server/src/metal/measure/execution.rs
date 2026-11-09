//! Task-owned timestamps. Queue transfer preserves the first publication;
//! reseating before a real return replaces the dispatch boundary.
use core::sync::atomic::{AtomicU64, Ordering};
pub(crate) struct Execution {
    ready: AtomicU64,
    seated: AtomicU64,
}
impl Execution {
    pub(crate) const fn new() -> Self {
        Self {
            ready: AtomicU64::new(0),
            seated: AtomicU64::new(0),
        }
    }
    pub(crate) fn enqueue(&self, now: u64) {
        let _ = self
            .ready
            .compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed);
    }
    pub(crate) fn dispatch(&self, now: u64) {
        self.seated.store(now, Ordering::Relaxed);
    }
    pub(crate) fn cancel(&self) {
        self.ready.store(0, Ordering::Relaxed);
        self.seated.store(0, Ordering::Relaxed);
    }
    pub(crate) fn finish(&self, now: u64) -> Option<(u64, u64, u64)> {
        let ready = self.ready.swap(0, Ordering::Relaxed);
        let seated = self.seated.swap(0, Ordering::Relaxed);
        if ready == 0 || seated < ready || now < seated {
            return None;
        }
        Some((seated - ready, now - seated, now - ready))
    }
}
