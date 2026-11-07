use alloc::vec::Vec;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::Entry;
use kernel_api::boot::ledger::Name;

use crate::basis::pend;
use crate::basis::pend::PendEntry;
use crate::basis::unit::task::Task;

use super::DeliveryError;
use super::Failure;
use super::valid;

pub(crate) struct Frozen {
    pub(super) entries: Vec<(Name, PendEntry)>,
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
                reason: PendFail::OoM,
                value: self,
            });
        }
        let mut entries = target.pend.entries.lock();
        if entries.try_reserve(self.entries.len()).is_err() {
            return Err(Failure {
                reason: PendFail::OoM,
                value: self,
            });
        }
        for (name, root) in self.entries {
            ledger.push(Entry::new(
                name,
                root.view().kind().into(),
                root.token().get(),
            ));
            entries.push(root);
        }
        drop(entries);
        pend::changed(target);
        drop(commit);
        drop(closed);
        Ok(ledger)
    }
}
