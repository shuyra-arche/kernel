//! Team exit closes admission without waiting for any hardware.
use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct Admission(AtomicBool);
impl Admission {
    pub(crate) const fn new() -> Self {
        Self(AtomicBool::new(false))
    }
    pub(crate) fn close(&self) {
        self.0.store(true, Ordering::Release);
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        self.close();
    }
}
