use ::core::marker::PhantomData;
use ::core::ops::{Deref, DerefMut};

use super::super::{Diagnostics, Environment};

use super::SpinLock;

pub struct SpinLockGuard<'a, T: ?Sized, E: Environment, D: Diagnostics> {
    pub(super) lock: &'a SpinLock<T, E, D>,
    _not_send: PhantomData<*const ()>,
    _trap: E::Guard,
    #[cfg(feature = "latency")]
    start: u64,
}

impl<'a, T: ?Sized, E: Environment, D: Diagnostics> SpinLockGuard<'a, T, E, D> {
    pub(super) fn new(lock: &'a SpinLock<T, E, D>, trap: E::Guard) -> Self {
        SpinLockGuard {
            lock,
            _not_send: PhantomData,
            _trap: trap,
            #[cfg(feature = "latency")]
            start: crate::metal::measure::stamp(),
        }
    }
}

impl<T: ?Sized, E: Environment, D: Diagnostics> Deref for SpinLockGuard<'_, T, E, D> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized, E: Environment, D: Diagnostics> DerefMut for SpinLockGuard<'_, T, E, D> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T: ?Sized, E: Environment, D: Diagnostics> Drop for SpinLockGuard<'_, T, E, D> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        D::released(self.lock as *const _ as *const () as usize);
        #[cfg(feature = "latency")]
        crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinHold, self.start);
        self.lock.turns.leave();
    }
}
