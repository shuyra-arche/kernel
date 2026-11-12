//! Ledger storage, transaction exclusion and unpublished registration rollback.
use super::{HoldTable, PendEntry};
use crate::basis::{
    name::{Key, Registry},
    unit::task::Task,
};
use crate::metal::sync::SpinLock;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use kernel_api::{abi::call::pend::PendFail, model::identity::PendId};

/// One Task's authority ledger. The transaction lock coordinates registrations,
/// borrow edges and source references; closure invalidates every hosted authority before asynchronous retirement.
/// Public PendIds index generational slots holding the single shared record.
pub(crate) struct Table {
    serial: SpinLock<()>,
    closed: Arc<AtomicBool>,
    pub(crate) entries: SpinLock<Registry<PendId, Arc<PendEntry>>>,
    pub(crate) heirs: SpinLock<Heirs>,
    pub(crate) references: HoldTable,
}

impl Table {
    pub(crate) fn new() -> Result<Self, PendFail> {
        Ok(Self {
            serial: SpinLock::new(()),
            closed: Arc::try_new(AtomicBool::new(false)).map_err(|_| PendFail::OutOfMemory)?,
            entries: SpinLock::new(Registry::default()),
            heirs: SpinLock::new(Heirs {
                entries: Vec::new(),
                reserved: 0,
            }),
            references: HoldTable::new(),
        })
    }

    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub(crate) fn lock(&self) -> crate::metal::sync::spin::SpinLockGuard<'_, ()> {
        self.serial.lock()
    }
    pub(crate) fn try_lock(&self) -> Option<crate::metal::sync::spin::SpinLockGuard<'_, ()>> {
        self.serial.try_lock()
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
        if self.entries.lock().len() >= 1024 {
            return Err(PendFail::OutOfMemory);
        }
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

pub(in super::super) fn with_pair<T>(
    a: &Task,
    b: &Task,
    run: impl FnOnce() -> T,
) -> Result<T, PendFail> {
    with_pair_uncommitted(a, b, || {
        let _commit = crate::basis::commit();
        Ok(run())
    })?
}

pub(in super::super) fn with_pair_uncommitted<T>(
    a: &Task,
    b: &Task,
    run: impl FnOnce() -> T,
) -> Result<T, PendFail> {
    let (first, second) = if a.ident.id.get() <= b.ident.id.get() {
        (a, b)
    } else {
        (b, a)
    };
    let _first = first.pend.try_lock().ok_or(PendFail::Busy)?;
    let _second = if first.ident.id == second.ident.id {
        None
    } else {
        Some(second.pend.try_lock().ok_or(PendFail::Busy)?)
    };
    Ok(run())
}
