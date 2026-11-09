//! Fixed logarithmic buckets; counters wrap only after 2^64 observations.
use core::sync::atomic::{AtomicU64, Ordering};
pub(super) struct Distribution {
    count: AtomicU64,
    max: AtomicU64,
    bins: [AtomicU64; 32],
}
impl Distribution {
    pub(super) const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            max: AtomicU64::new(0),
            bins: [const { AtomicU64::new(0) }; 32],
        }
    }
    pub(super) fn record(&self, ticks: u64) {
        // 0 -> bucket 0; [2^(i-1), 2^i-1] -> i; final bucket saturates.
        let bucket = (64 - ticks.leading_zeros() as usize).min(31);
        self.bins[bucket].fetch_add(1, Ordering::Relaxed);
        self.max.fetch_max(ticks, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
    }
    pub(super) fn snapshot(&self) -> (u64, u64, [u64; 32]) {
        (
            self.count.load(Ordering::Relaxed),
            self.max.load(Ordering::Relaxed),
            self.bins.each_ref().map(|v| v.load(Ordering::Relaxed)),
        )
    }
}
