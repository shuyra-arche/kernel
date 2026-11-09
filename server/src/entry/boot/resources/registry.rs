use ::core::fmt;
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::{Entry, Name};

use crate::basis::pend::PendEntry;
use crate::basis::unit::task::Task;

#[derive(Default)]
pub(crate) struct Registry {
    entries: Vec<(Name, Arc<PendEntry>)>,
}

impl Registry {
    pub(crate) fn region(&self, base: usize) -> Option<Arc<PendEntry>> {
        self.entries
            .iter()
            .find(|(name, _)| {
                *name == Name::Page(kernel_api::boot::ledger::Page::Region(base as u64))
            })
            .map(|(_, entry)| entry.clone())
    }
    pub(crate) fn register(&mut self, name: Name, root: PendEntry) -> Result<(), RegisterError> {
        // Boot grants access to Kernel-owned endpoints, not permission to
        // destroy them or inject synthetic hardware/fault notifications.
        use kernel_api::model::pend::{CloneRights, Operations, ShareRights};
        let mut limits = root.authority().limits();
        limits.operations = match root.target().kind() {
            Some(kernel_api::model::view::ViewKind::Clone) => {
                Operations::Clone(CloneRights::PULL | CloneRights::PEEK)
            }
            Some(kernel_api::model::view::ViewKind::Share) => {
                Operations::Share(ShareRights::OPEN | ShareRights::SHUT | ShareRights::ASSOCIATE)
            }
            None => limits.operations,
        };
        if let Err(reason) = root.narrow(limits, root.authority().window()) {
            return Err(Failure {
                reason,
                value: (name, root),
            });
        }
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
        for (name, root) in &self.entries {
            let entry = if let Name::Interrupt { .. } = name {
                let view = root
                    .target()
                    .sign()
                    .and_then(|s| crate::basis::room::interrupt::region(&s))
                    .and_then(|base| {
                        self.entries.iter().find(|(n, _)| {
                            *n == Name::Page(kernel_api::boot::ledger::Page::Region(base as u64))
                        })
                    })
                    .map(|(_, p)| p.token().get());
                let Some(view) = view else {
                    return Err(Failure {
                        reason: PendFail::Denied,
                        value: self,
                    });
                };
                Entry::pair(
                    *name,
                    kernel_api::model::view::ViewKind::Share,
                    view,
                    root.token().get(),
                )
            } else {
                Entry::new(*name, name.kind(), root.token().get())
            };
            ledger.push(entry);
        }
        for registration in registrations {
            registration.publish();
        }

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
        && (pend_entry
            .target()
            .kind()
            .is_some_and(|kind| name.kind() == kind.into())
            || matches!(name, Name::Port(id) if pend_entry.target().port().is_some_and(|p| p.id().get() as u64 == id))
            || matches!(name, Name::Interrupt { controller, source } if pend_entry.target().sign().is_some_and(|s| crate::basis::room::interrupt::identifies(&s, controller, source))))
        && pend_entry.alive()
        && pend_entry.sire().is_none()
        && pend_entry.authority().modifiable().is_some()
}
