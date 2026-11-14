//! Task-owned timestamps. Queue transfer preserves the first publication;
//! reseating before a real return replaces the dispatch boundary.
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
pub(crate) const INELIGIBLE: u64 = 1;
pub(crate) const BUDGET: u64 = 2;
pub(crate) const OUTRANKED: u64 = 4;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sample {
    pub(crate) queue_ticks: u64,
    pub(crate) dispatch_ticks: u64,
    pub(crate) total_ticks: u64,
    pub(crate) origin: usize,
    pub(crate) queue: usize,
    pub(crate) destination: usize,
    pub(crate) ordinary: usize,
    pub(crate) fixed: usize,
    pub(crate) entries: u64,
    pub(crate) selections: u64,
    pub(crate) rejected: u64,
}
pub(crate) struct Execution {
    ready: AtomicU64,
    seated: AtomicU64,
    origin: AtomicUsize,
    queue: AtomicUsize,
    destination: AtomicUsize,
    ordinary: AtomicUsize,
    fixed: AtomicUsize,
    entries: AtomicU64,
    selections: AtomicU64,
    rejected: AtomicU64,
}
impl Execution {
    pub(crate) const fn new() -> Self {
        Self {
            ready: AtomicU64::new(0),
            seated: AtomicU64::new(0),
            origin: AtomicUsize::new(0),
            queue: AtomicUsize::new(0),
            destination: AtomicUsize::new(0),
            ordinary: AtomicUsize::new(0),
            fixed: AtomicUsize::new(0),
            entries: AtomicU64::new(0),
            selections: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        }
    }
    pub(crate) fn enqueue(&self, now: u64, hart: usize, ordinary: usize, fixed: usize) {
        if self
            .ready
            .compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            self.origin.store(hart, Ordering::Relaxed);
        }
        self.queue.store(hart, Ordering::Relaxed);
        self.ordinary.store(ordinary, Ordering::Relaxed);
        self.fixed.store(fixed, Ordering::Relaxed);
        self.entries.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn reject(&self, reason: u64) {
        self.rejected.fetch_or(reason, Ordering::Relaxed);
    }
    pub(crate) fn dispatch(&self, now: u64, hart: usize) {
        self.seated.store(now, Ordering::Relaxed);
        self.destination.store(hart, Ordering::Relaxed);
        self.selections.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn cancel(&self) {
        self.ready.store(0, Ordering::Relaxed);
        self.seated.store(0, Ordering::Relaxed);
        self.entries.store(0, Ordering::Relaxed);
        self.selections.store(0, Ordering::Relaxed);
        self.rejected.store(0, Ordering::Relaxed);
    }
    pub(crate) fn finish(&self, now: u64) -> Option<Sample> {
        let ready = self.ready.swap(0, Ordering::Relaxed);
        let seated = self.seated.swap(0, Ordering::Relaxed);
        let sample = Sample {
            queue_ticks: seated.saturating_sub(ready),
            dispatch_ticks: now.saturating_sub(seated),
            total_ticks: now.saturating_sub(ready),
            origin: self.origin.load(Ordering::Relaxed),
            queue: self.queue.load(Ordering::Relaxed),
            destination: self.destination.load(Ordering::Relaxed),
            ordinary: self.ordinary.load(Ordering::Relaxed),
            fixed: self.fixed.load(Ordering::Relaxed),
            entries: self.entries.swap(0, Ordering::Relaxed),
            selections: self.selections.swap(0, Ordering::Relaxed),
            rejected: self.rejected.swap(0, Ordering::Relaxed),
        };
        (ready != 0 && seated >= ready && now >= seated).then_some(sample)
    }
}
