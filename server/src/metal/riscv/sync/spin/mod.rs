use ::core::cell::UnsafeCell;

use super::trap::TrapGuard;
use super::turn::Turns;

mod guard;

pub use guard::SpinLockGuard;

pub struct SpinLock<T: ?Sized> {
    turns: Turns,

    #[cfg(debug_assertions)]
    class: super::order::Class,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub(super) const fn new(_class: super::order::Class, val: T) -> Self {
        SpinLock {
            turns: Turns::new(),
            #[cfg(debug_assertions)]
            class: _class,
            data: UnsafeCell::new(val),
        }
    }
}

impl<T: ?Sized> SpinLock<T> {
    #[inline(never)]
    #[cfg_attr(feature = "latency", track_caller)]
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::deadlock_caller!();
        let trap = unsafe { TrapGuard::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        crate::metal::sync::deadlock_before!(self, caller);

        self.turns.enter();
        crate::metal::sync::deadlock_acquired!(self, caller);

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
    pub(super) fn lock_ordered<U>(&self, _lower: &SpinLockGuard<'_, U>) -> SpinLockGuard<'_, T> {
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::deadlock_caller!();
        let trap = unsafe { TrapGuard::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        crate::metal::sync::deadlock_before_pair!(
            self,
            _lower.lock,
            super::deadlock::Pair::Flat,
            caller
        );

        self.turns.enter();
        crate::metal::sync::deadlock_acquired_pair!(self, _lower.lock, caller);

        {
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinWait, start);
            SpinLockGuard::new(self, trap)
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "latency", track_caller)]
    pub(super) fn try_lock_ordered<U>(
        &self,
        _lower: &SpinLockGuard<'_, U>,
    ) -> Option<SpinLockGuard<'_, T>> {
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::deadlock_caller!();
        let trap = unsafe { TrapGuard::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();
        crate::metal::sync::deadlock_before_pair!(
            self,
            _lower.lock,
            super::deadlock::Pair::Flat,
            caller
        );
        if !self.turns.try_enter() {
            return None;
        }
        crate::metal::sync::deadlock_acquired_pair!(self, _lower.lock, caller);
        Some({
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinWait, start);
            SpinLockGuard::new(self, trap)
        })
    }

    #[inline(never)]
    #[cfg_attr(feature = "latency", track_caller)]
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::deadlock_caller!();
        let trap = unsafe { TrapGuard::save() };
        #[cfg(feature = "latency")]
        let start = crate::metal::measure::stamp();

        crate::metal::sync::deadlock_before!(self, caller);
        if !self.turns.try_enter() {
            return None;
        }
        crate::metal::sync::deadlock_acquired!(self, caller);

        Some({
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::SpinWait, start);
            SpinLockGuard::new(self, trap)
        })
    }
}

impl SpinLock<()> {
    pub(super) fn try_lock_ledger_peer(
        &self,
        _first: &SpinLockGuard<'_, ()>,
        first_id: usize,
        second_id: usize,
    ) -> Option<SpinLockGuard<'_, ()>> {
        assert!(first_id < second_id, "ledger pair identity order");
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::deadlock_caller!();
        let trap = unsafe { TrapGuard::save() };
        crate::metal::sync::deadlock_before_pair!(
            self,
            _first.lock,
            super::deadlock::Pair::Ledger {
                first: first_id,
                second: second_id
            },
            caller
        );
        if !self.turns.try_enter() {
            return None;
        }
        crate::metal::sync::deadlock_acquired_pair!(self, _first.lock, caller);
        Some(SpinLockGuard::new(self, trap))
    }
    pub(super) fn try_source_borrow(&self) -> Option<SpinLockGuard<'_, ()>> {
        #[cfg(debug_assertions)]
        let caller = crate::metal::sync::deadlock_caller!();
        let trap = unsafe { TrapGuard::save() };
        #[cfg(debug_assertions)]
        super::deadlock::before_source_borrow(self as *const _ as usize, caller);
        if !self.turns.try_enter() {
            return None;
        }
        #[cfg(debug_assertions)]
        super::deadlock::acquired(
            self as *const _ as usize,
            self.class,
            caller,
            super::deadlock::Kind::SourceBorrow,
        );
        Some(SpinLockGuard::new(self, trap))
    }
}
