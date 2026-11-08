use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::metal::generic::clock::{Comparator, Compare, DISARMED};

/// Owns the backend and the last confirmed hardware setting. All accesses are
/// serialized by the owning Hart with interrupts masked.
pub(crate) struct LocalTimer<B> {
    backend: B,
    armed: AtomicU64,
    known: AtomicBool,
}

impl<B: Comparator> LocalTimer<B> {
    pub(crate) fn new(backend: B) -> Result<Self, B::Error> {
        backend.program(Compare::OFF)?;
        Ok(Self {
            backend,
            armed: AtomicU64::new(DISARMED),
            known: AtomicBool::new(true),
        })
    }

    pub(crate) fn setting(&self) -> Option<Compare> {
        self.known
            .load(Ordering::Relaxed)
            .then(|| Compare::from_ticks(self.armed.load(Ordering::Relaxed)))
    }

    /// Returns whether a hardware write was needed. Even a request to restore
    /// the prior setting must reach the backend after a failed write.
    pub(crate) fn set(&self, setting: Compare) -> Result<bool, B::Error> {
        if self.setting() == Some(setting) {
            return Ok(false);
        }
        if let Err(error) = self.backend.program(setting) {
            self.known.store(false, Ordering::Relaxed);
            return Err(error);
        }
        self.armed.store(setting.ticks(), Ordering::Relaxed);
        self.known.store(true, Ordering::Relaxed);
        Ok(true)
    }
}
