//! Single-hart machine-mode fixture: interrupts stay disabled for the whole run.
//! This supplies Page's lock surface without importing the Kernel scheduler.
use core::{cell::RefCell, cell::RefMut};
#[allow(dead_code)]
#[path = "../../../../../src/basis/sync/order/mod.rs"]
pub(crate) mod order;
pub(crate) struct LegacyLock<P, T>(RefCell<T>, core::marker::PhantomData<fn() -> P>);
// SAFETY: this guest admits one hart and never enables interrupts.
unsafe impl<P, T: Send> Sync for LegacyLock<P, T> {}
impl<P, T> LegacyLock<P, T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(RefCell::new(value), core::marker::PhantomData)
    }
    pub(crate) fn lock(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }
}
