use ::core::marker::PhantomData;
use ::core::ops::{Deref, DerefMut};

use crate::metal::sync::trap::TrapGuard;

use super::SpinLock;

pub struct SpinLockGuard<'a, T: ?Sized> {
    lock: &'a SpinLock<T>,
    _not_send: PhantomData<*const ()>,
    _trap: TrapGuard,
    #[cfg(feature = "latency")]
    start: u64,
}

impl<'a, T: ?Sized> SpinLockGuard<'a, T> {
    pub(super) fn new(lock: &'a SpinLock<T>, trap: TrapGuard) -> Self {
        SpinLockGuard {
            lock,
            _not_send: PhantomData,
            _trap: trap,
            #[cfg(feature = "latency")]
            start: crate::metal::measure::stamp(),
        }
    }
}

impl<T: ?Sized> Deref for SpinLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T: ?Sized> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        crate::metal::sync::depend_release!(self.lock);
        #[cfg(feature = "latency")]
        crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinHold, self.start);
        self.lock.turns.leave();
    }
}
