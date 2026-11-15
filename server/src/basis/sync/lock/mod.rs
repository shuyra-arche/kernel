//! One actual FIFO exclusion instance. Static policies select storage and access;
//! a child guard borrows its parent and its target for the same lifetime.
mod guard;
mod ledger;
pub(crate) use ledger::LedgerSerialPairGuard;
mod pair;
pub(crate) mod reentry;
use super::order::{self, Constructible, Order};
use core::marker::PhantomData;
pub(crate) use guard::LockGuard;
pub(crate) use pair::LockPairGuard;
use reentry::{Accept, Policy, Reject};

#[repr(transparent)]
pub(crate) struct Lock<O: Order, R: Policy, T> {
    pub(super) raw: R::Raw<T>,
    class: PhantomData<O>,
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<O: Constructible, T> Lock<O, Reject, T> {
    #[inline(always)]
    pub(crate) const fn new(value: T) -> Self {
        Self {
            // SAFETY: the sealed Order/policy fixes this field's class.
            raw: unsafe { super::SpinLock::new(O::CLASS, value) },
            class: PhantomData,
        }
    }
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<O: Constructible, T> Lock<O, Accept, T> {
    #[inline(always)]
    pub(crate) const fn new(value: T) -> Self {
        Self {
            // SAFETY: the sealed Order/policy fixes this field's class.
            raw: unsafe { super::RelLock::new(O::CLASS, value) },
            class: PhantomData,
        }
    }
}
impl Lock<order::Commit, Accept, ()> {
    // Only sync can instantiate this class. The one static below owns it;
    // domain owners obtain a reference, never a constructor or a copied value.
    pub(super) const fn global() -> Self {
        Self {
            // SAFETY: the sealed Order/policy fixes this field's class.
            raw: unsafe { super::RelLock::new(order::Class::Commit, ()) },
            class: PhantomData,
        }
    }
}
#[allow(
    dead_code,
    reason = "complete typed acquisition protocol; owners migrate in audited stages"
)]
impl<O: Order, R: Policy, T> Lock<O, R, T> {
    /// # Safety
    /// Migration entry only. The enforcing owner must establish the audited
    /// execution/ancestor relation; new typed paths use Root or a real parent.
    #[inline(always)]
    pub(crate) unsafe fn legacy_lock(&self) -> R::Guard<'_, T> {
        R::lock(&self.raw)
    }
    /// # Safety
    /// Same ancestor restrictions as legacy_lock, including nonblocking entry.
    #[inline(always)]
    pub(crate) unsafe fn legacy_try_lock(&self) -> Option<R::Guard<'_, T>> {
        R::try_lock(&self.raw)
    }
    #[inline(always)]
    pub(super) fn acquire(&self) -> LockGuard<'_, O, R, T> {
        LockGuard::new(self, R::lock(&self.raw))
    }
    #[inline(always)]
    pub(super) fn try_acquire(&self) -> Option<LockGuard<'_, O, R, T>> {
        R::try_lock(&self.raw).map(|raw| LockGuard::new(self, raw))
    }
}
