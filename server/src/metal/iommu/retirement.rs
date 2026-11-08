//! Records belong to Kernel; owner exit is only an admission signal. No hardware
//! work, allocation or transfer of records is required in the owner's Drop.
use super::BindingId;
use super::Error;
use super::Hardware;
use super::IoSpace;
use crate::basis::cell::projection::admission::{Admission, Watch};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DomainId(BindingId);

struct Entry<H: Hardware> {
    id: DomainId,
    admission: Watch,
    space: IoSpace<H>,
    error: Option<Error>,
    completed: Arc<AtomicBool>,
}

pub(crate) struct Registry<H: Hardware> {
    entries: Vec<Entry<H>>,
    cursor: usize,
}

#[derive(Default, Debug)]
pub(crate) struct Progress {
    pub(crate) visited: usize,
    pub(crate) reclaimed: usize,
    pub(crate) failed: usize,
}

impl<H: Hardware> Registry<H> {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
        }
    }
    pub(crate) fn pending(&self) -> usize {
        self.entries.len()
    }

    /// Install before registering/publishing any data. Setup errors return the
    /// hardware object so the Kernel can still shut down its queues.
    pub(crate) fn attach(
        &mut self,
        owner: &Admission,
        hardware: H,
    ) -> Result<DomainId, (Error, H)> {
        let mut prepare = || {
            if owner.closed() {
                return Err(Error::Closed);
            }
            self.entries
                .try_reserve(1)
                .map_err(|_| Error::OutOfMemory)?;
            BindingId::allocate().map(DomainId)
        };
        let completed = match Arc::try_new(AtomicBool::new(false)) {
            Ok(value) => value,
            Err(_) => return Err((Error::OutOfMemory, hardware)),
        };
        let id = match prepare() {
            Ok(id) => id,
            Err(e) => return Err((e, hardware)),
        };
        self.entries.push(Entry {
            id,
            admission: owner.watch().clone(),
            space: IoSpace::new(hardware),
            error: None,
            completed,
        });
        Ok(id)
    }

    /// Serialize with progress under the same Kernel lock. The admission load
    /// linearizes against Admission::close; work admitted just before close is still
    /// retained and subsequently retired. No IoSpace reference escapes the call.
    pub(crate) fn with_space<T>(
        &mut self,
        owner: &Admission,
        id: DomainId,
        action: impl FnOnce(&mut IoSpace<H>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(Error::UnknownDomain)?;
        if !entry.admission.same(owner.watch()) {
            return Err(Error::UnknownDomain);
        }
        if entry.admission.closed() {
            return Err(Error::Closed);
        }
        action(&mut entry.space)
    }

    /// Kernel failure handling closes a retained record without Unit consent.
    pub(crate) fn close(&mut self, id: DomainId) -> Result<(), Error> {
        self.entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(Error::UnknownDomain)?
            .space
            .close()
    }

    /// Kernel-only queue observation; an exited Unit cannot prevent draining.
    pub(crate) fn fault(&mut self, id: DomainId) -> Result<Option<super::Fault>, Error> {
        self.entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(Error::UnknownDomain)?
            .space
            .fault()
    }

    pub(crate) fn completion(&self, id: DomainId) -> Option<Completion> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| Completion(entry.completed.clone()))
    }

    pub(crate) fn error(&self, id: DomainId) -> Result<Option<Error>, Error> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.error)
            .ok_or(Error::UnknownDomain)
    }

    /// Round-robin, at most budget visits and at most one per original entry.
    /// A stalled/failed domain never prevents another owner from progressing.
    /// Failed records stay enumerable; the next visit retries with a fresh batch.
    pub(crate) fn step(&mut self, budget: usize) -> Progress {
        let mut progress = Progress::default();
        let visits = budget.min(self.entries.len());
        for _ in 0..visits {
            self.cursor %= self.entries.len();
            let entry = &mut self.entries[self.cursor];
            progress.visited += 1;
            if entry.admission.closed() || entry.space.life() != super::Life::Open {
                match entry.space.advance_close() {
                    Ok(true) => {
                        entry.completed.store(true, Ordering::Release);
                        self.entries.remove(self.cursor);
                        progress.reclaimed += 1;
                        continue;
                    }
                    Ok(false) => {
                        entry.error = None;
                    }
                    Err(error) => {
                        entry.error = Some(error);
                        progress.failed += 1;
                    }
                }
            }
            self.cursor += 1;
        }
        progress
    }
}

/// Read-only receipt, minted only for an installed Kernel record. Disappearance
/// or a guessed ID cannot manufacture settlement.
pub(crate) struct Completion(Arc<AtomicBool>);
impl Completion {
    pub(crate) fn settled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
