use ::core::marker::PhantomData;
use ::core::ops::Deref;
use ::core::sync::atomic::Ordering;

use super::super::{Diagnostics, Environment};

use super::RelLock;
use super::owner::Owner;

pub struct RelLockGuard<'a, T: ?Sized, E: Environment, D: Diagnostics> {
    lock: &'a RelLock<T, E, D>,
    _not_send: PhantomData<*const ()>,
    _trap: E::Guard,
    #[cfg(feature = "latency")]
    start: u64,
}

impl<'a, T: ?Sized, E: Environment, D: Diagnostics> RelLockGuard<'a, T, E, D> {
    pub(super) fn new(lock: &'a RelLock<T, E, D>, trap: E::Guard) -> Self {
        RelLockGuard {
            lock,
            _not_send: PhantomData,
            _trap: trap,
            #[cfg(feature = "latency")]
            start: crate::metal::measure::stamp(),
        }
    }
}

impl<T: ?Sized, E: Environment, D: Diagnostics> Deref for RelLockGuard<'_, T, E, D> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized, E: Environment, D: Diagnostics> Drop for RelLockGuard<'_, T, E, D> {
    fn drop(&mut self) {
        let c = unsafe {
            let p = self.lock.count.get();
            *p -= 1;
            *p
        };
        #[cfg(feature = "latency")]
        crate::metal::measure::elapsed(crate::metal::measure::Metric::CommitHold, self.start);
        if c == 0 {
            #[cfg(feature = "latency")]
            if let Some(outer) = unsafe { (*self.lock.outer.get()).take() } {
                crate::metal::measure::complete(outer);
            }
            #[cfg(debug_assertions)]
            D::released(self.lock as *const _ as *const () as usize);
            self.lock.owner.store(Owner::FREE.word(), Ordering::Relaxed);
            // Publish all protected writes before the next reserved Hart may
            // enter. Recursive releases never advance the queue.
            self.lock.turns.leave();
        }
    }
}
