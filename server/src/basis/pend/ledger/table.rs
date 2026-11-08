//! Ledger storage, transaction exclusion and unpublished registration rollback.
use super::{HoldTable, PendEntry};
use crate::basis::{
    name::{Key, Registry},
    unit::task::Task,
};
use crate::metal::sync::SpinLock;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;
use kernel_api::{abi::call::pend::PendFail, model::identity::PendId};

/// One Task's authority ledger. The transaction lock coordinates registrations,
/// borrow edges and source references; version validates optimistic observers.
/// Public PendIds index generational slots holding the single shared record.
pub(crate) struct Table {
    serial: SpinLock<()>,
    pub(crate) version: AtomicUsize,
    pub(crate) entries: SpinLock<Registry<PendId, Arc<PendEntry>>>,
    pub(crate) heirs: SpinLock<Vec<(PendId, Weak<Task>, PendId)>>,
    pub(crate) references: HoldTable,
}

impl Table {
    pub(crate) fn new() -> Self {
        Self {
            serial: SpinLock::new(()),
            version: AtomicUsize::new(0),
            entries: SpinLock::new(Registry::default()),
            heirs: SpinLock::new(Vec::new()),
            references: HoldTable::new(),
        }
    }

    pub(crate) fn lock(&self) -> crate::metal::sync::spin::SpinLockGuard<'_, ()> {
        self.serial.lock()
    }

    /// Caller holds the ledger transaction lock until publication or rollback.
    /// The guard owns both the unpublished record and its reserved slot.
    pub(crate) fn prepare(&self, entry: Arc<PendEntry>) -> Result<Registration<'_>, PendFail> {
        let key = self
            .entries
            .lock()
            .reserve(entry.token())
            .ok_or(PendFail::OutOfMemory)?;
        Ok(Registration {
            table: self,
            key,
            entry: Some(entry),
        })
    }
}

#[must_use = "unpublished registrations roll back on drop"]
pub(crate) struct Registration<'a> {
    table: &'a Table,
    key: Key<PendId>,
    entry: Option<Arc<PendEntry>>,
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
