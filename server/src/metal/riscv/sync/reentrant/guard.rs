use ::core::marker::PhantomData;
use ::core::ops::Deref;
use ::core::sync::atomic::Ordering;

use crate::metal::sync::trap::TrapGuard;

use super::RelLock;
use super::owner::Owner;

pub struct RelLockGuard<'a, T: ?Sized> {
    lock: &'a RelLock<T>,
    _not_send: PhantomData<*const ()>,
    _trap: TrapGuard,
    #[cfg(feature = "latency")]
    start: u64,
}

impl<'a, T: ?Sized> RelLockGuard<'a, T> {
    pub(super) fn new(lock: &'a RelLock<T>, trap: TrapGuard) -> Self {
        RelLockGuard {
            lock,
            _not_send: PhantomData,
            _trap: trap,
            #[cfg(feature = "latency")]
            start: crate::metal::measure::stamp(),
        }
    }
}

impl<T: ?Sized> Deref for RelLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> Drop for RelLockGuard<'_, T> {
    fn drop(&mut self) {
        let c = unsafe {
            let p = self.lock.count.get();
            *p -= 1;
            *p
        };
        #[cfg(feature = "latency")]
        crate::metal::measure::elapsed(crate::metal::measure::Metric::CommitHold, self.start);
        if c == 0 {
            #[cfg(debug_assertions)]
            super::depend::release(self.lock as *const _ as *const () as usize);
            self.lock.owner.store(Owner::FREE.word(), Ordering::Relaxed);
            // Publish all protected writes before the next reserved Hart may
            // enter. Recursive releases never advance the queue.
            self.lock.serving.fetch_add(1, Ordering::Release);
        }
    }
}
