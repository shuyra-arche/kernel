use ::core::cell::UnsafeCell;
use ::core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(debug_assertions)]
use super::Admission;
use super::turn::Turns;
use super::{Diagnostics, Environment};

mod guard;
mod owner;

pub use guard::RelLockGuard;

use owner::Owner;

#[derive(Debug)]
pub struct RelLock<T: ?Sized, E: Environment, D: Diagnostics> {
    // One outstanding admission per Hart; recursive guards reuse its ticket.
    // Wrapping sequence numbers remain distinct with fewer than usize::MAX
    // outstanding Harts. A reservation cannot be cancelled or bypassed.
    turns: Turns<E>,
    diagnostics: core::marker::PhantomData<fn() -> D>,
    owner: AtomicUsize,
    count: UnsafeCell<usize>,
    #[cfg(feature = "latency")]
    outer: UnsafeCell<Option<crate::metal::measure::Transaction>>,

    #[cfg(debug_assertions)]
    class: D::Class,
    data: UnsafeCell<T>,
}

// Guards remain on their owning hart; recursive guards expose only shared access.
unsafe impl<T: ?Sized + Send, E: Environment, D: Diagnostics> Sync for RelLock<T, E, D> {}

impl<T, E: Environment, D: Diagnostics> RelLock<T, E, D> {
    /// # Safety
    /// Only the invariant owner may create storage with this diagnostic class.
    /// It must enforce unique instances and special admission rules where required.
    pub(crate) const unsafe fn new(_class: D::Class, val: T) -> Self {
        RelLock {
            turns: Turns::new(),
            diagnostics: core::marker::PhantomData,
            owner: AtomicUsize::new(0),
            count: UnsafeCell::new(0),
            #[cfg(feature = "latency")]
            outer: UnsafeCell::new(None),
            #[cfg(debug_assertions)]
            class: _class,
            data: UnsafeCell::new(val),
        }
    }
}

impl<T: ?Sized, E: Environment, D: Diagnostics> RelLock<T, E, D> {
    /// One acquisition attempt; a foreign owner is never waited on. Recursive
    /// acquisition stays on the same Hart and creates no second lockdep entry.
    /// A failed attempt drops TrapGuard and restores the caller's IRQ state.
    #[cfg_attr(feature = "latency", track_caller)]
    #[inline(never)]
    pub fn try_lock(&self) -> Option<RelLockGuard<'_, T, E, D>> {
        #[cfg(debug_assertions)]
        let caller = E::caller();
        let trap = unsafe { E::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        let me = Owner::of(E::owner());
        if Owner::from_word(self.owner.load(Ordering::Relaxed)) == me {
            self.reenter();
        } else {
            // Acquire observes the previous outer release. Reserve only an
            // empty queue: a failed try creates no abandoned ticket, and a
            // free owner between turns does not let this caller barge.
            #[cfg(debug_assertions)]
            D::before(
                self as *const _ as *const () as usize,
                self.class,
                caller,
                Admission::Ordinary,
            );
            if !self.turns.try_enter() {
                return None;
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
            #[cfg(debug_assertions)]
            D::acquired(
                self as *const _ as *const () as usize,
                self.class,
                caller,
                Admission::Ordinary,
            );
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
        self.owner.load(Ordering::Relaxed) == Owner::of(E::owner()).word()
    }

    #[cfg_attr(feature = "latency", track_caller)]
    #[inline(never)]
    pub fn lock(&self) -> RelLockGuard<'_, T, E, D> {
        #[cfg(debug_assertions)]
        let caller = E::caller();
        let trap = unsafe { E::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        let me = Owner::of(E::owner());

        #[cfg(debug_assertions)]
        if Owner::from_word(self.owner.load(Ordering::Relaxed)) != me {
            D::before(
                self as *const Self as *const () as usize,
                self.class,
                caller,
                Admission::Ordinary,
            );
        }

        if Owner::from_word(self.owner.load(Ordering::Relaxed)) == me {
            self.reenter();
        } else {
            self.turns.enter();
            self.owner.store(me.word(), Ordering::Relaxed);
            unsafe { *self.count.get() = 1 };
            #[cfg(feature = "latency")]
            unsafe {
                *self.outer.get() = Some(crate::metal::measure::transaction(
                    start,
                    core::panic::Location::caller(),
                ));
            }
            #[cfg(debug_assertions)]
            D::acquired(
                self as *const _ as *const () as usize,
                self.class,
                caller,
                Admission::Ordinary,
            );
        }

        {
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::CommitWait, start);
            RelLockGuard::new(self, trap)
        }
    }
}
