#![allow(dead_code, unused_imports)]
//! Namespace/resource-owner backend. Reuse the production typed Lock, policies,
//! LegacyLock and order declarations over host mutexes. This shell proves no
//! RV64 IRQ/FIFO behavior; tests/sync uses the real atomic primitives instead.
use core::{
    cell::Cell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};
use std::sync::{Mutex, MutexGuard, ReentrantLock, ReentrantLockGuard};
#[path = "../../src/basis/sync/legacy.rs"]
mod legacy;
#[path = "../../src/basis/sync/lock/mod.rs"]
pub(crate) mod lock;
#[path = "../../src/basis/sync/order/mod.rs"]
pub(crate) mod order;
pub(crate) use legacy::LegacyLock;
pub(crate) use reentrant::{RelLock, RelLockGuard};
pub(crate) use spin::{SpinLock, SpinLockGuard};
pub(crate) type TestLock<T> = LegacyLock<order::ProbeRegistry, T>;
pub(crate) use std::sync::OnceLock;
pub(crate) enum Admission {
    Borrow,
}
pub(crate) mod diagnostics {
    pub(crate) enum Pair {
        Flat,
        Ledger { first: usize, second: usize },
    }
}
pub(crate) type LegacyGuard<'a, T> = HostGuard<'a, T>;
thread_local! {
 static DEPTH:Cell<usize>=const{Cell::new(0)};
 static SINGLE:Cell<bool>=const{Cell::new(false)};
}
pub(crate) struct HostGuard<'a, T> {
    lock: &'a spin::SpinLock<T>,
    guard: MutexGuard<'a, T>,
}
impl<T> Deref for HostGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}
impl<T> DerefMut for HostGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}
impl<T> Drop for HostGuard<'_, T> {
    fn drop(&mut self) {
        DEPTH.set(DEPTH.get() - 1);
    }
}
pub(crate) mod spin {
    use super::*;
    pub(crate) type SpinLockGuard<'a, T> = HostGuard<'a, T>;
    pub(crate) struct SpinLock<T>(Mutex<T>);
    impl<T> SpinLock<T> {
        pub(crate) const unsafe fn new(_: order::Class, value: T) -> Self {
            Self(Mutex::new(value))
        }
        fn before() {
            assert!(
                !SINGLE.get() || DEPTH.get() == 0,
                "Pend storage guards overlap in a single-storage audit scope"
            );
        }
        pub(crate) fn lock(&self) -> HostGuard<'_, T> {
            Self::before();
            let guard = self.0.lock().unwrap();
            DEPTH.set(DEPTH.get() + 1);
            HostGuard { lock: self, guard }
        }
        pub(crate) fn try_lock(&self) -> Option<HostGuard<'_, T>> {
            Self::before();
            let guard = self.0.try_lock().ok()?;
            DEPTH.set(DEPTH.get() + 1);
            Some(HostGuard { lock: self, guard })
        }
        pub(crate) fn lock_ordered<U>(
            &self,
            first: &HostGuard<'_, U>,
            _: diagnostics::Pair,
        ) -> HostGuard<'_, T> {
            assert_ne!(self as *const _ as usize, first.lock as *const _ as usize);
            self.lock()
        }
        pub(crate) fn try_lock_ordered<U>(
            &self,
            first: &HostGuard<'_, U>,
            _: diagnostics::Pair,
        ) -> Option<HostGuard<'_, T>> {
            assert_ne!(self as *const _ as usize, first.lock as *const _ as usize);
            self.try_lock()
        }
    }
    impl<T> SpinLock<T> {
        pub(crate) fn try_admit(&self, _: Admission) -> Option<HostGuard<'_, T>> {
            self.try_lock()
        }
    }
}

pub(crate) mod reentrant {
    use super::*;
    pub(crate) type RelLockGuard<'a, T> = ReentrantLockGuard<'a, T>;
    pub(crate) struct RelLock<T>(ReentrantLock<T>);
    impl<T> RelLock<T> {
        pub(crate) const unsafe fn new(_: order::Class, value: T) -> Self {
            Self(ReentrantLock::new(value))
        }
        pub(crate) fn lock(&self) -> RelLockGuard<'_, T> {
            self.0.lock()
        }
        pub(crate) fn try_lock(&self) -> Option<RelLockGuard<'_, T>> {
            self.0.try_lock()
        }
    }
}
// Namespace suites never construct a Task execution boundary. Its opaque
// signature lets the imported owner API compile; actual Root construction and
// lifetime failures are tested from the production module in tests/sync.
pub(crate) struct Root<'a>(PhantomData<&'a mut ()>);
impl Root<'_> {
    pub(crate) fn lock<'a, O: order::RootOrder, R: lock::reentry::Policy, T>(
        &'a mut self,
        target: &'a lock::Lock<O, R, T>,
    ) -> lock::LockGuard<'a, O, R, T> {
        target.acquire()
    }
}
static COMMIT: lock::Lock<order::Commit, lock::reentry::Accept, ()> = lock::Lock::global();
pub(crate) fn global_commit() -> &'static lock::Lock<order::Commit, lock::reentry::Accept, ()> {
    &COMMIT
}
pub(crate) fn commit() -> ReentrantLockGuard<'static, ()> {
    unsafe { COMMIT.legacy_lock() }
}
struct Audit(bool);
impl Drop for Audit {
    fn drop(&mut self) {
        SINGLE.set(self.0);
    }
}
pub(crate) fn single<R>(body: impl FnOnce() -> R) -> R {
    assert_eq!(DEPTH.get(), 0);
    let _audit = Audit(SINGLE.replace(true));
    body()
}
