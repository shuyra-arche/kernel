//! Source-local, bounded-space discovery of actual Nest memberships.
//! Entries retain only exact bindings; this index grants no authority and
//! never treats disappearance from a table as hardware retirement.
use alloc::{sync::Arc, vec::Vec};
use kernel_api::abi::call::pend::PendFail;

pub(crate) struct Members<T> {
    entries: Vec<(usize, Arc<T>)>,
    sequence: usize,
    prepared: usize,
}

impl<T> Members<T> {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            sequence: 0,
            prepared: 0,
        }
    }

    /// Reserve one infallible publication. Several pending reservations cannot
    /// overcommit capacity or exhaust the non-reusing sequence at commit time.
    pub(crate) fn prepare(&mut self) -> Result<(), PendFail> {
        let prepared = self.prepared.checked_add(1).ok_or(PendFail::OutOfMemory)?;
        self.sequence
            .checked_add(prepared)
            .ok_or(PendFail::OutOfMemory)?;
        self.entries
            .try_reserve(prepared)
            .map_err(|_| PendFail::OutOfMemory)?;
        self.prepared = prepared;
        Ok(())
    }

    pub(crate) fn cancel(&mut self) {
        self.prepared = self.prepared.checked_sub(1).expect("prepared membership");
    }

    pub(crate) fn publish(&mut self, member: Arc<T>) {
        assert!(self.prepared != 0 && self.entries.len() < self.entries.capacity());
        self.prepared -= 1;
        self.sequence = self.sequence.checked_add(1).expect("reserved sequence");
        self.entries.push((self.sequence, member));
    }

    /// A fixed upper boundary prevents new publications from making a scan
    /// unbounded. Order is publication order, not pointer or reusable slot order.
    pub(crate) fn upper(&self) -> usize {
        self.sequence
    }

    pub(crate) fn after(&self, cursor: usize, end: usize) -> Option<(usize, Arc<T>)> {
        let at = self.entries.partition_point(|(sequence, _)| *sequence <= cursor);
        self.entries
            .get(at)
            .filter(|(sequence, _)| *sequence <= end)
            .map(|(sequence, member)| (*sequence, member.clone()))
    }

    pub(crate) fn first(&self) -> Option<Arc<T>> {
        self.entries.first().map(|(_, member)| member.clone())
    }

    pub(crate) fn remove(&mut self, member: &Arc<T>) {
        self.entries
            .retain(|(_, known)| !Arc::ptr_eq(known, member));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.prepared == 0
    }
}
