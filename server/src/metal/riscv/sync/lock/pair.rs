//! Same-class admission is an instance protocol, never a category self edge.
use super::super::spin::SpinLockGuard;
use super::{
    Lock, LockGuard,
    reentry::{Policy, Reject},
};
use crate::metal::sync::order::{Before, Order, PairOrder};
use core::marker::PhantomData;

// Rust drops fields in declaration order: always second, then first, restoring
// the first saved IRQ state only after both physical turns have been released.
enum Held<'a, A, B> {
    AB {
        second: SpinLockGuard<'a, B>,
        first: SpinLockGuard<'a, A>,
    },
    BA {
        second: SpinLockGuard<'a, A>,
        first: SpinLockGuard<'a, B>,
    },
}
#[must_use = "both pair instances share one RAII lifetime"]
pub(crate) struct LockPairGuard<'a, O: Order, A, B> {
    held: Held<'a, A, B>,
    parent: PhantomData<&'a mut O>,
    not_send: PhantomData<*const ()>,
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<'a, O: PairOrder, A, B> LockPairGuard<'a, O, A, B> {
    pub(in super::super) fn acquire(a: &'a Lock<O, Reject, A>, b: &'a Lock<O, Reject, B>) -> Self {
        let a_at = &a.raw as *const _ as usize;
        let b_at = &b.raw as *const _ as usize;
        assert_ne!(a_at, b_at, "pair requires distinct lock instances");
        let held = if a_at < b_at {
            let first = a.raw.lock();
            let second = b.raw.lock_ordered(&first);
            Held::AB { second, first }
        } else {
            let first = b.raw.lock();
            let second = a.raw.lock_ordered(&first);
            Held::BA { second, first }
        };
        Self {
            held,
            parent: PhantomData,
            not_send: PhantomData,
        }
    }
    pub(in super::super) fn try_acquire(
        a: &'a Lock<O, Reject, A>,
        b: &'a Lock<O, Reject, B>,
    ) -> Option<Self> {
        let a_at = &a.raw as *const _ as usize;
        let b_at = &b.raw as *const _ as usize;
        assert_ne!(a_at, b_at, "pair requires distinct lock instances");
        let held = if a_at < b_at {
            let first = a.raw.try_lock()?;
            let second = b.raw.try_lock_ordered(&first)?;
            Held::AB { second, first }
        } else {
            let first = b.raw.try_lock()?;
            let second = a.raw.try_lock_ordered(&first)?;
            Held::BA { second, first }
        };
        Some(Self {
            held,
            parent: PhantomData,
            not_send: PhantomData,
        })
    }
    /// # Safety
    /// Staged owner entry only: the caller establishes the audited ancestors.
    /// The address/instance protocol and IRQ release order remain enforced.
    pub(crate) unsafe fn legacy(a: &'a Lock<O, Reject, A>, b: &'a Lock<O, Reject, B>) -> Self {
        Self::acquire(a, b)
    }
    pub(crate) fn values(&mut self) -> (&mut A, &mut B) {
        match &mut self.held {
            Held::AB { second, first } => (first, second),
            Held::BA { second, first } => (second, first),
        }
    }
    pub(crate) fn lock<'g, N: Order, R: Policy, T>(
        &'g mut self,
        target: &'g Lock<N, R, T>,
    ) -> LockGuard<'g, N, R, T>
    where
        O: Before<N>,
    {
        target.acquire()
    }
}
