use ::core::cell::UnsafeCell;

use super::turn::Turns;
use super::{Admission, Diagnostics, Environment};

mod guard;

pub use guard::SpinLockGuard;

pub struct SpinLock<T: ?Sized, E: Environment, D: Diagnostics> {
    turns: Turns<E>,
    diagnostics: core::marker::PhantomData<fn() -> D>,

    #[cfg(debug_assertions)]
    class: D::Class,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send, E: Environment, D: Diagnostics> Sync for SpinLock<T, E, D> {}

impl<T, E: Environment, D: Diagnostics> SpinLock<T, E, D> {
    /// # Safety
    /// Only the invariant owner may create storage with this diagnostic class.
    /// It must enforce unique instances and special admission rules where required.
    pub(crate) const unsafe fn new(_class: D::Class, val: T) -> Self {
        SpinLock {
            turns: Turns::new(),
            diagnostics: core::marker::PhantomData,
            #[cfg(debug_assertions)]
            class: _class,
            data: UnsafeCell::new(val),
        }
    }
}

impl<T: ?Sized, E: Environment, D: Diagnostics> SpinLock<T, E, D> {
    #[inline(never)]
    #[cfg_attr(feature = "latency", track_caller)]
    pub fn lock(&self) -> SpinLockGuard<'_, T, E, D> {
        #[cfg(debug_assertions)]
        let caller = E::caller();
        let trap = unsafe { E::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        #[cfg(debug_assertions)]
        D::before(
            self as *const _ as *const () as usize,
            self.class,
            caller,
            Admission::Ordinary,
        );

        self.turns.enter();
        #[cfg(debug_assertions)]
        D::acquired(
            self as *const _ as *const () as usize,
            self.class,
            caller,
            Admission::Ordinary,
        );

        {
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinWait, start);
            SpinLockGuard::new(self, trap)
        }
    }

    /// Second member of the actual Flat pair, witnessed by its first guard.
    /// Only the shared RAII pair may call this and controls both release order
    /// and the common lifetime; ordinary same-class acquisition has no edge.
    #[inline(never)]
    #[cfg_attr(feature = "latency", track_caller)]
    pub(crate) fn lock_ordered<U>(
        &self,
        _lower: &SpinLockGuard<'_, U, E, D>,
        _policy: D::Pair,
    ) -> SpinLockGuard<'_, T, E, D> {
        #[cfg(debug_assertions)]
        let caller = E::caller();
        let trap = unsafe { E::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        #[cfg(debug_assertions)]
        D::before(
            self as *const _ as *const () as usize,
            self.class,
            caller,
            Admission::Pair {
                predecessor: _lower.lock as *const _ as *const () as usize,
                policy: _policy,
            },
        );

        self.turns.enter();
        #[cfg(debug_assertions)]
        D::acquired(
            self as *const _ as *const () as usize,
            self.class,
            caller,
            Admission::Pair {
                predecessor: _lower.lock as *const _ as *const () as usize,
                policy: _policy,
            },
        );

        {
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinWait, start);
            SpinLockGuard::new(self, trap)
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "latency", track_caller)]
    pub(crate) fn try_lock_ordered<U>(
        &self,
        _lower: &SpinLockGuard<'_, U, E, D>,
        _policy: D::Pair,
    ) -> Option<SpinLockGuard<'_, T, E, D>> {
        #[cfg(debug_assertions)]
        let caller = E::caller();
        let trap = unsafe { E::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        #[cfg(debug_assertions)]
        D::before(
            self as *const _ as *const () as usize,
            self.class,
            caller,
            Admission::Pair {
                predecessor: _lower.lock as *const _ as *const () as usize,
                policy: _policy,
            },
        );
        if !self.turns.try_enter() {
            return None;
        }
        #[cfg(debug_assertions)]
        D::acquired(
            self as *const _ as *const () as usize,
            self.class,
            caller,
            Admission::Pair {
                predecessor: _lower.lock as *const _ as *const () as usize,
                policy: _policy,
            },
        );
        Some({
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinWait, start);
            SpinLockGuard::new(self, trap)
        })
    }

    #[inline(never)]
    #[cfg_attr(feature = "latency", track_caller)]
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T, E, D>> {
        self.try_admit(Admission::Ordinary)
    }

    #[inline(always)]
    #[cfg_attr(feature = "latency", track_caller)]
    pub(crate) fn try_admit(
        &self,
        _admission: Admission<D::Pair>,
    ) -> Option<SpinLockGuard<'_, T, E, D>> {
        #[cfg(debug_assertions)]
        let caller = E::caller();
        let trap = unsafe { E::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();

        #[cfg(debug_assertions)]
        D::before(
            self as *const _ as *const () as usize,
            self.class,
            caller,
            _admission,
        );
        if !self.turns.try_enter() {
            return None;
        }
        #[cfg(debug_assertions)]
        D::acquired(
            self as *const _ as *const () as usize,
            self.class,
            caller,
            _admission,
        );

        Some({
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinWait, start);
            SpinLockGuard::new(self, trap)
        })
    }
}
