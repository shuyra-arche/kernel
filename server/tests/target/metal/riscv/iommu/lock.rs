//! Single-hart machine-mode fixture: interrupts stay disabled for the whole run.
//! This supplies Page's lock surface without importing the Kernel scheduler.
use core::{cell::RefCell, cell::RefMut};
pub(crate) struct SpinLock<T>(RefCell<T>);
// SAFETY: this guest admits one hart and never enables interrupts.
unsafe impl<T: Send> Sync for SpinLock<T> {}
impl<T> SpinLock<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(RefCell::new(value))
    }
    pub(crate) fn lock(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }
}
