//! Resources held across CPU translation invalidation.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{Invalidation, tickets::Tickets};
use crate::metal::mmu::Table;
use crate::metal::mmu::fence::{Completed, Scope};

use super::super::{Asid, Mapping};
use crate::basis::nest::mapping::publication::Publication;

/// PTE edits and their obligations stay together until the covering fence completes.
/// Address tickets become claimable in the same transition as the covering fence.
pub(in crate::basis::nest) struct Retirement {
    invalidation: Invalidation,
    scope: Scope,
    pub(in crate::basis::nest) tickets: Tickets,
    pub(in crate::basis::nest) tables: Vec<Table>,
    pub(in crate::basis::nest) cells: Vec<Publication>,
    pub(in crate::basis::nest) mappings: Vec<Arc<Mapping>>,
}

impl Retirement {
    pub(in crate::basis::nest) fn new() -> Self {
        Self {
            invalidation: Invalidation::Clean,
            scope: Scope::AddressSpace,
            tickets: Tickets::new(),
            tables: Vec::new(),
            cells: Vec::new(),
            mappings: Vec::new(),
        }
    }

    pub(in crate::basis::nest) fn invalidation(&self) -> Invalidation {
        self.invalidation
    }

    pub(in crate::basis::nest) fn mark_dirty(&mut self, global: bool) {
        assert!(
            !self.invalidation.is_fencing(),
            "cannot mutate a fencing translation batch"
        );
        self.scope = self.scope.include(global);
        self.invalidation = self.invalidation.mark_dirty();
    }

    /// Select the covering fence before releasing the Nest lock to run it.
    pub(in crate::basis::nest) fn begin(&mut self, asid: Asid) -> Asid {
        self.invalidation = self.invalidation.begin();
        self.target(asid)
    }

    fn target(&self, asid: Asid) -> Asid {
        match self.scope {
            Scope::AddressSpace => asid,
            Scope::Global => Asid::kernel(),
        }
    }

    /// Keep all obligations available for the retry.
    pub(in crate::basis::nest) fn fail(&mut self) {
        self.invalidation = self.invalidation.fail();
    }

    /// Acknowledge publications while holding the Nest lock.
    pub(in crate::basis::nest) fn complete(
        &mut self,
        asid: Asid,
        proof: Completed<Asid>,
    ) -> Fenced {
        assert!(self.invalidation.is_fencing());
        assert_eq!(proof.target(), self.target(asid));
        for mapping in &self.mappings {
            for (_, publication) in &mut mapping.state.lock().published {
                publication.complete();
            }
        }
        self.tickets.settle();
        self.invalidation = self.invalidation.complete();
        self.scope = Scope::AddressSpace;
        Fenced {
            tables: core::mem::take(&mut self.tables),
            cells: core::mem::take(&mut self.cells),
            mappings: core::mem::take(&mut self.mappings),
        }
    }
}

/// Resources from a completed fence. Release them outside the Nest lock.
pub(in crate::basis::nest) struct Fenced {
    tables: Vec<Table>,
    cells: Vec<Publication>,
    mappings: Vec<Arc<Mapping>>,
}

impl Fenced {
    pub(in crate::basis::nest) fn release(self) {
        drop(self.tables);
        for retained in self.cells {
            // SAFETY: Retirement constructed this batch only after the fence
            // covering the removed PTEs completed.
            unsafe { retained.settle() };
        }
        for mapping in &self.mappings {
            mapping
                .state
                .lock()
                .settle_deleted_memos(mapping.initial_start().get());
        }
        drop(self.mappings);
    }
}
