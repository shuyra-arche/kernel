//! Actual Pend owner with host mutex storage. A bounded audit scope diagnoses
//! overlapping Pend storage guards before blocking; it is not RV64 lockdep.
use std::{
    cell::Cell,
    ops::{Deref, DerefMut},
    sync::{Mutex, MutexGuard},
};
thread_local! {
    static HELD: Cell<usize> = const { Cell::new(0) };
    static SINGLE: Cell<bool> = const { Cell::new(false) };
}
pub(crate) struct SpinLock<T>(Mutex<T>);
pub(crate) struct SpinLockGuard<'a, T>(MutexGuard<'a, T>);
impl<T> SpinLock<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }
    pub(crate) fn lock(&self) -> SpinLockGuard<'_, T> {
        assert!(
            !SINGLE.get() || HELD.get() == 0,
            "Pend storage guards overlap in a single-storage audit scope"
        );
        let guard = self.0.lock().unwrap();
        HELD.set(HELD.get() + 1);
        SpinLockGuard(guard)
    }
}
impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        HELD.set(HELD.get() - 1);
    }
}
struct Audit(bool);
impl Drop for Audit {
    fn drop(&mut self) {
        SINGLE.set(self.0);
    }
}
pub(super) fn single<R>(body: impl FnOnce() -> R) -> R {
    assert_eq!(HELD.get(), 0);
    let _audit = Audit(SINGLE.replace(true));
    body()
}
