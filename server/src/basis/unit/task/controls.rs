//! Caller-owned mappings and their round-robin retirement cursor. An unsettled
//! mapping stays registered while other mappings still get a cleanup turn.
use crate::basis::nest::{Nest, NestError};
use crate::metal::sync::SpinLock;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use kernel_api::model::{cell::MappingControl, identity::TaskId};

pub(crate) struct Controls {
    inner: SpinLock<Entries>,
}
struct Entries {
    records: Vec<(Weak<Nest>, usize)>,
    next: usize,
}
impl Controls {
    pub(super) const fn new() -> Self {
        Self {
            inner: SpinLock::new(Entries {
                records: Vec::new(),
                next: 0,
            }),
        }
    }
    pub(crate) fn insert(&self, nest: &Arc<Nest>, id: usize) -> Result<(), NestError> {
        let mut entries = self.inner.lock();
        entries
            .records
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        entries.records.push((Arc::downgrade(nest), id));
        Ok(())
    }
    pub(crate) fn remove(&self, id: usize) {
        let mut entries = self.inner.lock();
        if let Some(at) = entries.records.iter().position(|(_, known)| *known == id) {
            entries.records.swap_remove(at);
            // Revisit a record moved into the removed position.
            entries.next = entries.next.min(at);
        }
    }
    /// Attempt only one mapping. Hardware failure never removes its record;
    /// moving the cursor prevents one busy device from blocking unrelated work.
    pub(crate) fn retire(&self, actor: TaskId) -> Result<(), NestError> {
        let (nest, id) = {
            let mut entries = self.inner.lock();
            if entries.records.is_empty() {
                return Ok(());
            }
            let at = entries.next % entries.records.len();
            entries.next = at + 1;
            entries.records[at].clone()
        };
        let target = nest.upgrade().and_then(|nest| {
            let entry = nest.mappings.registered(id)?;
            let owned = entry.control.as_ref().is_some_and(|control| {
                control.control().0 == actor && control.scope() == MappingControl::Caller
            });
            owned.then_some((nest, entry))
        });
        if let Some((nest, entry)) = target {
            if !nest.settled(id) {
                if let Some(mapping) = entry.mapping() {
                    let _ = nest.dispose(mapping.clone());
                }
                nest.retry_invalidation()?;
            }
            if !nest.settled(id) {
                return Err(NestError::Busy);
            }
            // Revalidates settlement under COMMIT. Only Nest::settle removes
            // our record; a competing publication must not lose its controller.
            nest.retire_mapping(id);
        } else {
            // An absent Nest/registration cannot be retried. Removing this
            // stale record does not manufacture a hardware completion.
            self.remove(id);
        }
        if self.inner.lock().records.is_empty() {
            Ok(())
        } else {
            Err(NestError::Busy)
        }
    }
}
