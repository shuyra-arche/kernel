//! TaskId pair protocol. The safe domain entry takes actual Tasks, not arbitrary
//! caller-selected ID/lock tuples. This mechanism owns both real FIFO guards.
use super::super::spin::SpinLockGuard;
use super::{
    Lock, LockGuard,
    reentry::{Policy, Reject},
};
use crate::metal::sync::order::{self, Before, Order};
use core::marker::PhantomData;
#[must_use]
pub(crate) struct LedgerSerialPairGuard<'a> {
    _second: Option<SpinLockGuard<'a, ()>>,
    _first: SpinLockGuard<'a, ()>,
    parent: PhantomData<&'a mut ()>,
}
impl<'a> LedgerSerialPairGuard<'a> {
    /// # Safety
    /// Only the ledger owner couples these keys to the actual Tasks containing
    /// the given immutable serial fields and establishes the ancestor scope.
    pub(crate) unsafe fn legacy(
        a: &'a Lock<order::LedgerSerial, Reject, ()>,
        a_id: usize,
        b: &'a Lock<order::LedgerSerial, Reject, ()>,
        b_id: usize,
    ) -> Option<Self> {
        let same = core::ptr::eq(a, b);
        assert_eq!(
            same,
            a_id == b_id,
            "Task identity must denote its actual serial instance"
        );
        let (first, first_id, second, second_id) = if a_id <= b_id {
            (a, a_id, b, b_id)
        } else {
            (b, b_id, a, a_id)
        };
        let first = first.raw.try_lock()?;
        let second = if same {
            None
        } else {
            Some(
                second
                    .raw
                    .try_lock_ledger_peer(&first, first_id, second_id)?,
            )
        };
        Some(Self {
            _second: second,
            _first: first,
            parent: PhantomData,
        })
    }
    pub(crate) fn lock<'g, O: Order, R: Policy, T>(
        &'g mut self,
        target: &'g Lock<O, R, T>,
    ) -> LockGuard<'g, O, R, T>
    where
        order::LedgerSerial: Before<O>,
    {
        target.acquire()
    }
}
impl Lock<order::LedgerSerial, Reject, ()> {
    /// # Safety
    /// Only Source::borrow's enumerated one-shot try protocol may invert the
    /// global Commit edge. It must hold that exact Commit, enter no further
    /// ledger/first-time Commit, and release this guard before hardware work.
    pub(crate) unsafe fn try_source_borrow(&self) -> Option<SpinLockGuard<'_, ()>> {
        self.raw.try_source_borrow()
    }
}
