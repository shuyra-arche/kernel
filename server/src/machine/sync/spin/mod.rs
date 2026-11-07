use ::core::cell::UnsafeCell;
use ::core::sync::atomic::AtomicBool;
use ::core::sync::atomic::Ordering;

use super::depend;
use super::trap::TrapGuard;

mod guard;

pub use guard::SpinLockGuard;

pub struct SpinLock<T: ?Sized> {
    locked: AtomicBool,

    #[cfg(debug_assertions)]
    level: Option<depend::Level>,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(val: T) -> Self {
        SpinLock {
            locked: AtomicBool::new(false),
            #[cfg(debug_assertions)]
            level: None,
            data: UnsafeCell::new(val),
        }
    }

    pub const fn new_level(_level: depend::Level, val: T) -> Self {
        SpinLock {
            locked: AtomicBool::new(false),
            #[cfg(debug_assertions)]
            level: Some(_level),
            data: UnsafeCell::new(val),
        }
    }
}

impl<T: ?Sized> SpinLock<T> {
    #[inline(never)]
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        #[cfg(debug_assertions)]
        let caller = crate::machine::sync::depend_enter!(self);
        let trap = unsafe { TrapGuard::save() };
        crate::machine::sync::depend_check!(self, caller);

        while self.locked.swap(true, Ordering::Acquire) {
            crate::machine::stop::hush();
            ::core::hint::spin_loop();
        }
        crate::machine::sync::depend_acquire!(self, caller);

        SpinLockGuard::new(self, trap)
    }

    /// 共持同级对的第二把：只在已持 `lower` 的前提下取，且按地址序在 `lower` 之后。
    ///
    /// 只给 `sync` 子树里的 `with_pair` 用：它不对外，所以没人能拿了第二把守卫
    /// 又先放低锁，把闭包里那对守卫的生命周期拆开。
    #[inline(never)]
    pub(super) fn lock_ordered<U>(&self, _lower: &SpinLock<U>) -> SpinLockGuard<'_, T> {
        #[cfg(debug_assertions)]
        let caller = crate::machine::sync::depend_enter!(self);
        let trap = unsafe { TrapGuard::save() };
        crate::machine::sync::depend_check_peer!(self, _lower, caller);

        while self.locked.swap(true, Ordering::Acquire) {
            crate::machine::stop::hush();
            ::core::hint::spin_loop();
        }
        crate::machine::sync::depend_acquire!(self, caller);

        SpinLockGuard::new(self, trap)
    }

    #[inline(never)]
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        #[cfg(debug_assertions)]
        let caller = crate::machine::sync::depend_enter!(self);
        let trap = unsafe { TrapGuard::save() };

        if self.locked.swap(true, Ordering::Acquire) {
            return None;
        }
        crate::machine::sync::depend_acquire!(self, caller);

        Some(SpinLockGuard::new(self, trap))
    }
}

// 分配器机制只认这一条原生 unit guard：层级、TrapGuard、IRQ 恢复、!Send 与 lockdep
// 全部留在本类型里，不经过任何包装锁。

impl<T> SpinLock<T> {
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(&mut *self.lock())
    }
}
