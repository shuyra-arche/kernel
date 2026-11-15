use super::{
    Lock, LockPairGuard,
    reentry::{Accept, Policy, Reject},
};
use crate::basis::sync::order::{Before, Order, PairOrder};
use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

#[must_use = "a lock guard retains exclusion and its parent's scope"]
pub(crate) struct LockGuard<'a, O: Order, R: Policy, T: 'a> {
    raw: R::Guard<'a, T>,
    #[allow(dead_code, reason = "exact-instance witness for Accept reentry")]
    target: &'a Lock<O, R, T>,
    parent: PhantomData<&'a mut ()>,
    not_send: PhantomData<*const ()>,
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<'a, O: Order, R: Policy, T> LockGuard<'a, O, R, T> {
    #[inline(always)]
    pub(super) fn new(target: &'a Lock<O, R, T>, raw: R::Guard<'a, T>) -> Self {
        Self {
            raw,
            target,
            parent: PhantomData,
            not_send: PhantomData,
        }
    }
    #[inline(always)]
    pub(crate) fn lock<'g, N: Order, S: Policy, U>(
        &'g mut self,
        target: &'g Lock<N, S, U>,
    ) -> LockGuard<'g, N, S, U>
    where
        O: Before<N>,
    {
        target.acquire()
    }
    #[inline(always)]
    pub(crate) fn try_lock<'g, N: Order, S: Policy, U>(
        &'g mut self,
        target: &'g Lock<N, S, U>,
    ) -> Option<LockGuard<'g, N, S, U>>
    where
        O: Before<N>,
    {
        target.try_acquire()
    }
    pub(crate) fn pair<'g, N: PairOrder, A, B>(
        &'g mut self,
        a: &'g Lock<N, Reject, A>,
        b: &'g Lock<N, Reject, B>,
    ) -> LockPairGuard<'g, N, A, B>
    where
        O: Before<N>,
    {
        LockPairGuard::acquire(a, b)
    }
    pub(crate) fn try_pair<'g, N: PairOrder, A, B>(
        &'g mut self,
        a: &'g Lock<N, Reject, A>,
        b: &'g Lock<N, Reject, B>,
    ) -> Option<LockPairGuard<'g, N, A, B>>
    where
        O: Before<N>,
    {
        LockPairGuard::try_acquire(a, b)
    }
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<'a, O: Order, T> LockGuard<'a, O, Accept, T> {
    /// The witness is this guard's exact target. No supplied class, address,
    /// Hart ID or different Lock can substitute for that real ancestor.
    pub(crate) fn reenter(&mut self) -> LockGuard<'_, O, Accept, T> {
        self.target.acquire()
    }
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<O: Order, R: Policy, T> Deref for LockGuard<'_, O, R, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.raw
    }
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<O: Order, T> DerefMut for LockGuard<'_, O, Reject, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.raw
    }
}
