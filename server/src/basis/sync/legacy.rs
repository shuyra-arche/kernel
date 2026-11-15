//! Enumerated migration entries. Every field has a real semantic Order and uses
//! the same Lock storage/policy; these owner methods still rely on the audited
//! caller chain rather than accepting a typed Root. Debug checks all instances.
use super::{
    SpinLockGuard,
    lock::{Lock, reentry::Reject},
    order::{Constructible, Order},
};

#[repr(transparent)]
pub(crate) struct LegacyLock<O: Order, T>(Lock<O, Reject, T>);
impl<O: Constructible, T> LegacyLock<O, T> {
    #[inline(always)]
    pub(crate) const fn new(value: T) -> Self {
        Self(Lock::<O, Reject, T>::new(value))
    }
}
impl<O: Order, T> LegacyLock<O, T> {
    #[inline(always)]
    pub(crate) fn lock(&self) -> SpinLockGuard<'_, T> {
        // SAFETY: this is an explicitly enumerated migration field; its owner
        // and ancestor obligations are documented in synchronization/README.
        unsafe { self.0.legacy_lock() }
    }
    #[inline(always)]
    pub(crate) fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        // SAFETY: same migration boundary; failed admission retains no turn.
        unsafe { self.0.legacy_try_lock() }
    }
    pub(crate) fn with<R>(&self, body: impl FnOnce(&mut T) -> R) -> R {
        body(&mut self.lock())
    }
    /// # Safety
    /// Only the enforcing owner may expose its actual typed Lock to its
    /// enumerated legacy methods. repr(transparent) preserves that exact
    /// instance, its protected state, physical turn and lifetime.
    #[inline(always)]
    pub(crate) unsafe fn from_lock(lock: &Lock<O, Reject, T>) -> &Self {
        unsafe { &*(lock as *const Lock<O, Reject, T> as *const Self) }
    }
}
