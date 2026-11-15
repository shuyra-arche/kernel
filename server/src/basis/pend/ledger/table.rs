//! Ledger storage, transaction exclusion and unpublished registration rollback.
use super::admission::{Admission, Charge};
use super::{HoldTable, PendEntry};
use crate::basis::{
    name::{Key, Registry},
    unit::task::Task,
};
use crate::metal::sync::{
    LegacyLock, Root,
    lock::{LedgerSerialPairGuard, Lock, LockGuard, reentry::Reject},
    order,
};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use kernel_api::{
    abi::call::pend::PendFail,
    model::identity::{PendId, TaskId},
};

/// One Task's authority ledger. The transaction lock coordinates registrations,
/// borrow edges and source references; closure invalidates every hosted authority before asynchronous retirement.
/// Public PendIds index generational slots holding the single shared record.
pub(crate) struct Table {
    serial: Lock<order::LedgerSerial, Reject, ()>,
    closed: Arc<AtomicBool>,
    admission: Arc<Admission>,
    pub(crate) entries:
        LegacyLock<crate::metal::sync::order::LedgerEntries, Registry<PendId, Arc<PendEntry>>>,
    pub(crate) heirs: LegacyLock<crate::metal::sync::order::LedgerHeirs, Heirs>,
    pub(crate) references: HoldTable,
}

impl Table {
    pub(crate) fn new(owner: TaskId) -> Result<Self, PendFail> {
        Ok(Self {
            serial: Lock::<order::LedgerSerial, Reject, ()>::new(()),
            closed: Arc::try_new(AtomicBool::new(false)).map_err(|_| PendFail::OutOfMemory)?,
            admission: Arc::try_new(Admission::new(owner)).map_err(|_| PendFail::OutOfMemory)?,
            entries: LegacyLock::new(Registry::default()),
            heirs: LegacyLock::new(Heirs {
                entries: Vec::new(),
                reserved: 0,
            }),
            references: HoldTable::new(),
        })
    }

    pub(crate) fn after<'a>(
        &'a self,
        root: &'a mut Root<'_>,
    ) -> LockGuard<'a, order::LedgerSerial, Reject, ()> {
        root.lock(&self.serial)
    }

    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub(crate) fn lock(&self) -> crate::metal::sync::LegacyGuard<'_, ()> {
        // SAFETY: audited legacy ledger transaction entry.
        unsafe { self.serial.legacy_lock() }
    }
    pub(crate) fn try_lock(&self) -> Option<crate::metal::sync::LegacyGuard<'_, ()>> {
        // SAFETY: nonblocking form of the audited ledger transaction entry.
        unsafe { self.serial.legacy_try_lock() }
    }

    /// The sole audited reverse-try path; never used by ordinary admission.
    pub(crate) unsafe fn try_source_borrow(
        &self,
    ) -> Option<crate::metal::sync::LegacyGuard<'_, ()>> {
        unsafe { self.serial.try_source_borrow() }
    }

    pub(crate) fn prepare_heir(
        &self,
        parent: PendId,
        child: Weak<Task>,
        token: PendId,
    ) -> Result<HeirRegistration<'_>, PendFail> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PendFail::Dead);
        }
        self.heirs.lock().prepare()?;
        Ok(HeirRegistration {
            table: self,
            entry: Some((parent, child, token)),
        })
    }

    /// Caller holds the ledger transaction lock until publication or rollback.
    /// The guard owns both the unpublished record and its reserved slot.
    pub(crate) fn prepare(&self, entry: Arc<PendEntry>) -> Result<Registration<'_>, PendFail> {
        if self.closed.load(Ordering::Acquire) {
            return Err(PendFail::Dead);
        }
        entry.authority().held_by(&self.closed)?;
        let charge = self.admission.reserve(entry.sender())?;
        let key = self
            .entries
            .lock()
            .reserve(entry.token())
            .ok_or(PendFail::OutOfMemory)?;
        Ok(Registration {
            table: self,
            key,
            entry: Some(entry),
            charge: Some(charge),
        })
    }
}

pub(crate) struct Heirs {
    entries: Vec<(PendId, Weak<Task>, PendId)>,
    reserved: usize,
}

impl core::ops::Deref for Heirs {
    type Target = [(PendId, Weak<Task>, PendId)];

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl Heirs {
    fn prepare(&mut self) -> Result<(), PendFail> {
        if self.entries.len() + self.reserved >= 1024 {
            return Err(PendFail::OutOfMemory);
        }
        self.entries
            .try_reserve(self.reserved + 1)
            .map_err(|_| PendFail::OutOfMemory)?;
        self.reserved += 1;
        Ok(())
    }

    fn publish(&mut self, entry: (PendId, Weak<Task>, PendId)) {
        assert!(self.reserved != 0);
        self.reserved -= 1;
        let at = self
            .entries
            .partition_point(|(source, _, _)| source.get() <= entry.0.get());
        self.entries.insert(at, entry);
    }

    fn cancel(&mut self) {
        assert!(self.reserved != 0);
        self.reserved -= 1;
    }

    pub(super) fn remove(&mut self, at: usize) -> (PendId, Weak<Task>, PendId) {
        self.entries.remove(at)
    }
}

#[must_use = "unpublished heirs roll back on drop"]
pub(crate) struct HeirRegistration<'a> {
    table: &'a Table,
    entry: Option<(PendId, Weak<Task>, PendId)>,
}

impl HeirRegistration<'_> {
    pub(crate) fn publish(mut self) {
        self.table
            .heirs
            .lock()
            .publish(self.entry.take().expect("prepared heir"));
    }
}

impl Drop for HeirRegistration<'_> {
    fn drop(&mut self) {
        if self.entry.is_some() {
            self.table.heirs.lock().cancel();
        }
    }
}

#[must_use = "unpublished registrations roll back on drop"]
pub(crate) struct Registration<'a> {
    table: &'a Table,
    key: Key<PendId>,
    entry: Option<Arc<PendEntry>>,
    charge: Option<Charge>,
}
impl Registration<'_> {
    /// All fallible domain work must finish before committing the registration.
    pub(crate) fn publish(self) {
        self.commit(|publish| publish());
    }

    /// Hold the ledger storage lock before entering another commit owner (for
    /// example a Clone queue). The callback may fail before publishing; after
    /// calling publish exactly once, its remaining effects must be infallible.
    /// Rejection drops the storage lock before cancelling the reservation.
    pub(crate) fn commit<R>(mut self, apply: impl FnOnce(&mut dyn FnMut()) -> R) -> R {
        let mut entries = self.table.entries.lock();
        apply(&mut || {
            let entry = self.entry.take().expect("unpublished registration");
            entry.charge(self.charge.take().expect("reserved registration capacity"));
            assert!(entries.publish(entry.token(), self.key, entry).is_ok());
        })
    }
}
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        if let Some(entry) = &self.entry {
            assert!(self.table.entries.lock().cancel(entry.token(), self.key));
        }
    }
}

/// Acquire the real Task owners by stable, non-reused TaskId. Matching IDs
/// require the same actual Task; an accidental reused/fabricated identity fails
/// before any turn is reserved. The same Task follows the single-lock branch.
pub(crate) fn pair<'a>(a: &'a Task, b: &'a Task) -> Result<LedgerSerialPairGuard<'a>, PendFail> {
    if (a.ident.id == b.ident.id) != core::ptr::eq(a, b) {
        return Err(PendFail::Denied);
    }
    // SAFETY: each immutable key and serial reference comes from that exact
    // Task. These are enumerated legacy transaction roots; Debug cross-checks
    // every live ancestor. No arbitrary key/lock tuple is a safe caller input.
    unsafe {
        LedgerSerialPairGuard::legacy(
            &a.pend.serial,
            a.ident.id.get(),
            &b.pend.serial,
            b.ident.id.get(),
        )
    }
    .ok_or(PendFail::Busy)
}
/// The typed entry also borrows the actual execution-boundary Root.
#[allow(
    dead_code,
    reason = "typed Task-pair entry verified by the owner and UI suites"
)]
pub(crate) fn pair_after<'a>(
    _root: &'a mut Root<'_>,
    a: &'a Task,
    b: &'a Task,
) -> Result<LedgerSerialPairGuard<'a>, PendFail> {
    pair(a, b)
}
