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
    owner: AtomicUsize,
    count: UnsafeCell<usize>,

    #[cfg(debug_assertions)]
    level: Option<depend::Level>,
    data: UnsafeCell<T>,
}

// Guards remain on their owning hart; recursive guards expose only shared access.
unsafe impl<T: ?Sized + Send> Sync for RelLock<T> {}

impl<T> RelLock<T> {
    pub const fn new_level(_level: depend::Level, val: T) -> Self {
        RelLock {
            owner: AtomicUsize::new(0),
            count: UnsafeCell::new(0),
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
    #[inline(never)]
    pub fn try_lock(&self) -> Option<RelLockGuard<'_, T>> {
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::depend_enter!(self);
        let trap = unsafe { TrapGuard::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        let me = Owner::of(hart::hart_id());
        match self.owner.compare_exchange(
            Owner::FREE.word(),
            me.word(),
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                crate::metal::sync::depend_check!(self, caller);
                unsafe { *self.count.get() = 1 };
                crate::metal::sync::depend_acquire!(self, caller);
            }
            Err(owner) if Owner::from_word(owner) == me => unsafe {
                *self.count.get() = (*self.count.get())
                    .checked_add(1)
                    .expect("recursion overflow");
            },
            Err(_) => return None,
        }
        #[cfg(feature = "latency")]
        crate::metal::measure::elapsed(crate::metal::measure::Metric::CommitWait, start);
        Some(RelLockGuard::new(self, trap))
    }

    #[cfg(debug_assertions)]
    pub(crate) fn held_by_current(&self) -> bool {
        self.owner.load(Ordering::Relaxed) == Owner::of(hart::hart_id()).word()
    }

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

        loop {
            match self.owner.compare_exchange(
                Owner::FREE.word(),
                me.word(),
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    unsafe { *self.count.get() = 1 };
                    #[cfg(debug_assertions)]
                    depend::acquire(
                        self as *const Self as *const () as usize,
                        self.level,
                        caller,
                    );
                    break;
                }
                Err(cur) if Owner::from_word(cur) == me => {
                    unsafe {
                        *self.count.get() = (*self.count.get())
                            .checked_add(1)
                            .expect("recursion overflow")
                    };
                    break;
                }
                Err(_) => {
                    while self.owner.load(Ordering::Relaxed) != Owner::FREE.word() {
                        crate::metal::stop::hush();
                        crate::metal::interrupt::window::poll();
                        ::core::hint::spin_loop();
                    }
                }
            }
        }

        {
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::CommitWait, start);
            RelLockGuard::new(self, trap)
        }
    }
}
