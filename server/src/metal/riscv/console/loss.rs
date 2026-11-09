use core::sync::atomic::{AtomicUsize, Ordering};
/// Diagnostic loss is independent of the user DebugPut byte-count contract.
pub(super) struct Loss(AtomicUsize);
impl Loss {
    pub(super) const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }
    pub(super) fn record(&self, requested: usize, accepted: Option<usize>) {
        let accepted = accepted.filter(|n| *n <= requested).unwrap_or(0);
        let lost = requested - accepted;
        let _ = self
            .0
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_add(lost))
            });
    }
    pub(super) fn take(&self) -> usize {
        self.0.swap(0, Ordering::Relaxed)
    }
}
