//! One scope established at a real Task call boundary. Domain owners cannot
//! construct its Boundary, call its factory, clone it or send it to another Hart.
use super::{
    lock::{
        Lock, LockGuard, LockPairGuard,
        reentry::{Policy, Reject},
    },
    order::{PairOrder, RootOrder},
};
use crate::metal::riscv::switcher::trap::Boundary;
use core::marker::PhantomData;

#[must_use]
pub(crate) struct Root<'boundary> {
    boundary: PhantomData<&'boundary mut Boundary>,
    not_send: PhantomData<*const ()>,
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<'boundary> Root<'boundary> {
    /// # Safety
    /// Only the trampoline's Task-call entry may use its private Boundary.
    /// No guard or borrowed state may survive the return to the trampoline.
    pub(in super::super) unsafe fn enter(_boundary: &'boundary mut Boundary) -> Self {
        #[cfg(debug_assertions)]
        super::deadlock::root();
        Self {
            boundary: PhantomData,
            not_send: PhantomData,
        }
    }
    #[inline(always)]
    pub(crate) fn lock<'a, O: RootOrder, R: Policy, T>(
        &'a mut self,
        target: &'a Lock<O, R, T>,
    ) -> LockGuard<'a, O, R, T> {
        target.acquire()
    }
    #[inline(always)]
    pub(crate) fn try_lock<'a, O: RootOrder, R: Policy, T>(
        &'a mut self,
        target: &'a Lock<O, R, T>,
    ) -> Option<LockGuard<'a, O, R, T>> {
        target.try_acquire()
    }
    pub(crate) fn pair<'a, O: PairOrder + RootOrder, A, B>(
        &'a mut self,
        a: &'a Lock<O, Reject, A>,
        b: &'a Lock<O, Reject, B>,
    ) -> LockPairGuard<'a, O, A, B> {
        LockPairGuard::acquire(a, b)
    }
    pub(crate) fn try_pair<'a, O: PairOrder + RootOrder, A, B>(
        &'a mut self,
        a: &'a Lock<O, Reject, A>,
        b: &'a Lock<O, Reject, B>,
    ) -> Option<LockPairGuard<'a, O, A, B>> {
        LockPairGuard::try_acquire(a, b)
    }
}
