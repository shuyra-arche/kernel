use alloc::vec::Vec;
use kernel_api::{abi::call::pend::PendFail, boot::ledger::Name};

use crate::basis::work::pend::PendEntry;

use super::{Failure, FreezeError, Frozen, RegisterError, valid};

#[derive(Default)]
pub(crate) struct Registry {
    entries: Vec<(Name, PendEntry)>,
}

impl Registry {
    pub(crate) fn register(&mut self, name: Name, root: PendEntry) -> Result<(), RegisterError> {
        let reason = if !valid(name, &root) || self.entries.iter().any(|(n, _)| *n == name) {
            Some(PendFail::Denied)
        } else if self.entries.try_reserve(1).is_err() {
            Some(PendFail::OoM)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(Failure {
                reason,
                value: (name, root),
            });
        }
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
