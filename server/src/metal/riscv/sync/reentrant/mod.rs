use ::core::cell::UnsafeCell;
use ::core::sync::atomic::{AtomicUsize, Ordering};

use crate::metal::hart;

use super::depend;
use super::trap::TrapGuard;

mod guard;
mod owner;

pub use guard::RelLockGuard;

use owner::Owner;

#[derive(Debug)]
pub struct RelLock<T: ?Sized> {
    // One outstanding admission per Hart; recursive guards reuse its ticket.
    // Wrapping sequence numbers remain distinct with fewer than usize::MAX
    // outstanding Harts. A reservation cannot be cancelled or bypassed.
    next: AtomicUsize,
    serving: AtomicUsize,
    owner: AtomicUsize,
    count: UnsafeCell<usize>,
    #[cfg(feature = "latency")]
    outer: UnsafeCell<Option<crate::metal::measure::Transaction>>,

    #[cfg(debug_assertions)]
    level: Option<depend::Level>,
    data: UnsafeCell<T>,
}

// Guards remain on their owning hart; recursive guards expose only shared access.
unsafe impl<T: ?Sized + Send> Sync for RelLock<T> {}

impl<T> RelLock<T> {
    pub const fn new_level(_level: depend::Level, val: T) -> Self {
        RelLock {
            next: AtomicUsize::new(0),
            serving: AtomicUsize::new(0),
            owner: AtomicUsize::new(0),
            count: UnsafeCell::new(0),
            #[cfg(feature = "latency")]
            outer: UnsafeCell::new(None),
            #[cfg(debug_assertions)]
            level: Some(_level),
            data: UnsafeCell::new(val),
        }
    }
}

impl<T: ?Sized> RelLock<T> {
    /// One acquisition attempt; a foreign owner is never waited on. Recursive
    /// acquisition stays on the same Hart and creates no second lockdep entry.
    /// A failed attempt drops TrapGuard and restores the caller's IRQ state.
    #[cfg_attr(feature = "latency", track_caller)]
    #[inline(never)]
    pub fn try_lock(&self) -> Option<RelLockGuard<'_, T>> {
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::depend_enter!(self);
        let trap = unsafe { TrapGuard::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        let me = Owner::of(hart::hart_id());
        if Owner::from_word(self.owner.load(Ordering::Relaxed)) == me {
            self.reenter();
        } else {
            // Acquire observes the previous outer release. Reserve only an
            // empty queue: a failed try creates no abandoned ticket, and a
            // free owner between turns does not let this caller barge.
            let serving = self.serving.load(Ordering::Acquire);
            self.next
                .compare_exchange(
                    serving,
                    serving.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .ok()?;
            crate::metal::sync::depend_check!(self, caller);
            self.owner.store(me.word(), Ordering::Relaxed);
            unsafe { *self.count.get() = 1 };
            #[cfg(feature = "latency")]
            unsafe {
                *self.outer.get() = Some(crate::metal::measure::transaction(
                    start,
                    core::panic::Location::caller(),
                ));
            }
            crate::metal::sync::depend_acquire!(self, caller);
        }
        #[cfg(feature = "latency")]
        crate::metal::measure::elapsed(crate::metal::measure::Metric::CommitWait, start);
        Some(RelLockGuard::new(self, trap))
    }

    fn reenter(&self) {
        unsafe {
            *self.count.get() = (*self.count.get())
                .checked_add(1)
                .expect("recursion overflow");
        }
    }

    #[cfg(debug_assertions)]
    pub(crate) fn held_by_current(&self) -> bool {
        self.owner.load(Ordering::Relaxed) == Owner::of(hart::hart_id()).word()
    }

    #[cfg_attr(feature = "latency", track_caller)]
    #[inline(never)]
    pub fn lock(&self) -> RelLockGuard<'_, T> {
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::depend_enter!(self);
        let trap = unsafe { TrapGuard::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        let me = Owner::of(hart::hart_id());

        #[cfg(debug_assertions)]
        if Owner::from_word(self.owner.load(Ordering::Relaxed)) != me {
            depend::check(
                self as *const Self as *const () as usize,
                self.level,
                caller,
            );
        }

        if Owner::from_word(self.owner.load(Ordering::Relaxed)) == me {
            self.reenter();
        } else {
            let ticket = self.next.fetch_add(1, Ordering::Relaxed);
            while self.serving.load(Ordering::Acquire) != ticket {
                crate::metal::stop::hush();
                crate::metal::interrupt::window::poll();
                ::core::hint::spin_loop();
            }
            self.owner.store(me.word(), Ordering::Relaxed);
            unsafe { *self.count.get() = 1 };
            #[cfg(feature = "latency")]
            unsafe {
                *self.outer.get() = Some(crate::metal::measure::transaction(
                    start,
                    core::panic::Location::caller(),
                ));
            }
            crate::metal::sync::depend_acquire!(self, caller);
        }

        {
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::CommitWait, start);
            RelLockGuard::new(self, trap)
        }
    }
}
