use ::core::fmt;
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::{Entry, Name};

use crate::basis::pend::{self, PendEntry};
use crate::basis::unit::task::Task;

#[derive(Default)]
pub(crate) struct Registry {
    entries: Vec<(Name, Arc<PendEntry>)>,
}

impl Registry {
    pub(crate) fn register(&mut self, name: Name, root: PendEntry) -> Result<(), RegisterError> {
        let reason = if !valid(name, &root) || self.entries.iter().any(|(n, _)| *n == name) {
            Some(PendFail::Denied)
        } else if self.entries.try_reserve(1).is_err() {
            Some(PendFail::OutOfMemory)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(Failure {
                reason,
                value: (name, root),
            });
        }
        let mut shared = match Arc::<PendEntry>::try_new_uninit() {
            Ok(shared) => shared,
            Err(_) => {
                return Err(Failure {
                    reason: PendFail::OutOfMemory,
                    value: (name, root),
                });
            }
        };
        Arc::get_mut(&mut shared)
            .expect("private registration")
            .write(root);
        // SAFETY: the unique allocation was initialized above.
        let root = unsafe { shared.assume_init() };
        self.entries.push((name, root));
        Ok(())
    }
    pub(crate) fn freeze(self) -> Result<Frozen, FreezeError> {
        if self
            .entries
            .iter()
            .any(|(name, pend_entry)| !valid(*name, pend_entry))
        {
            return Err(Failure {
                reason: PendFail::Denied,
                value: self,
            });
        }
        Ok(Frozen {
            entries: self.entries,
        })
    }
}

pub(crate) struct Frozen {
    entries: Vec<(Name, Arc<PendEntry>)>,
}

impl Frozen {
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn authority(self, target: &Task) -> Result<Vec<Entry>, DeliveryError> {
        let closed = target.pend.lock();
        let commit = crate::basis::commit();
        if !target.held() || self.entries.iter().any(|(n, p)| !valid(*n, p)) {
            return Err(Failure {
                reason: PendFail::Denied,
                value: self,
            });
        }
        let mut ledger = Vec::new();
        if ledger.try_reserve_exact(self.entries.len()).is_err() {
            return Err(Failure {
                reason: PendFail::OutOfMemory,
                value: self,
            });
        }
        let mut registrations = Vec::new();
        if registrations.try_reserve_exact(self.entries.len()).is_err() {
            return Err(Failure {
                reason: PendFail::OutOfMemory,
                value: self,
            });
        }
        for (_, root) in &self.entries {
            match target.pend.prepare(root.clone()) {
                Ok(registration) => registrations.push(registration),
                Err(reason) => {
                    return Err(Failure {
                        reason,
                        value: self,
                    });
                }
            }
        }
        for ((name, root), registration) in self.entries.into_iter().zip(registrations) {
            ledger.push(Entry::new(
                name,
                root.view().kind().into(),
                root.token().get(),
            ));
            registration.publish();
        }
        pend::changed(target);
        drop(commit);
        drop(closed);
        Ok(ledger)
    }
}

pub(crate) struct Failure<T> {
    pub reason: PendFail,
    pub value: T,
}

impl<T> Failure<T> {
    pub(crate) fn into_parts(self) -> (PendFail, T) {
        (self.reason, self.value)
    }
}

impl<T> fmt::Debug for Failure<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(f)
    }
}

pub(crate) type RegisterError = Failure<(Name, PendEntry)>;
pub(crate) type FreezeError = Failure<Registry>;
pub(crate) type DeliveryError = Failure<Frozen>;

fn valid(name: Name, pend_entry: &PendEntry) -> bool {
    let mut bytes = [0u8; kernel_api::boot::ledger::NAME_LEN];
    name.store(&mut bytes);
    Name::fetch(&bytes) == Some(name)
        && name.kind() == pend_entry.view().kind().into()
        && pend_entry.alive()
        && pend_entry.sire().is_none()
        && pend_entry.authority().modifiable().is_some()
        && pend_entry.authority().modifiable().is_some()
}
