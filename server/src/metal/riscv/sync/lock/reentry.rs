//! Sealed, static storage policies. Accept never provides mutable dereference.
use super::super::{reentrant, spin};
use core::ops::Deref;
mod sealed {
    pub trait Sealed {}
}
pub(crate) struct Reject;
pub(crate) struct Accept;
impl sealed::Sealed for Reject {}
impl sealed::Sealed for Accept {}
pub(crate) trait Policy: sealed::Sealed {
    type Raw<T>;
    type Guard<'a, T: 'a>: Deref<Target = T>;
    fn lock<T>(raw: &Self::Raw<T>) -> Self::Guard<'_, T>;
    fn try_lock<T>(raw: &Self::Raw<T>) -> Option<Self::Guard<'_, T>>;
}
impl Policy for Reject {
    type Raw<T> = spin::SpinLock<T>;
    type Guard<'a, T: 'a> = spin::SpinLockGuard<'a, T>;
    #[inline(always)]
    fn lock<T>(raw: &Self::Raw<T>) -> Self::Guard<'_, T> {
        raw.lock()
    }
    #[inline(always)]
    fn try_lock<T>(raw: &Self::Raw<T>) -> Option<Self::Guard<'_, T>> {
        raw.try_lock()
    }
}
impl Policy for Accept {
    type Raw<T> = reentrant::RelLock<T>;
    type Guard<'a, T: 'a> = reentrant::RelLockGuard<'a, T>;
    #[inline(always)]
    fn lock<T>(raw: &Self::Raw<T>) -> Self::Guard<'_, T> {
        raw.lock()
    }
    #[inline(always)]
    fn try_lock<T>(raw: &Self::Raw<T>) -> Option<Self::Guard<'_, T>> {
        raw.try_lock()
    }
}
