//! Resources held across CPU translation invalidation.

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::machine::mmu::riscv::synchronize::{Invalidation, Scope};
use crate::machine::mmu::riscv::table::TableNode;

use super::super::{Asid, Mapping};
use crate::basis::nest::mapping::publication::Publication;

/// PTE edits and their obligations stay together until the covering fence completes.
/// Address-occupancy tickets have a separate lifetime and remain in NestState.
pub(in crate::basis::nest) struct Retirement {
    invalidation: Invalidation,
    scope: Scope,
    pub(in crate::basis::nest) tables: Vec<TableNode>,
    pub(in crate::basis::nest) cells: Vec<Publication>,
    pub(in crate::basis::nest) mappings: Vec<Arc<Mapping>>,
}

impl Retirement {
    pub(in crate::basis::nest) fn new() -> Self {
        Self {
            invalidation: Invalidation::Clean,
            scope: Scope::AddressSpace,
            tables: Vec::new(),
            cells: Vec::new(),
            mappings: Vec::new(),
        }
    }

    pub(in crate::basis::nest) fn invalidation(&self) -> Invalidation {
        self.invalidation
    }

    pub(in crate::basis::nest) fn mark_dirty(&mut self, global: bool) {
        self.scope = self.scope.include(global);
        self.invalidation = self.invalidation.mark_dirty();
    }

    /// Select the covering fence before releasing the Nest lock to run it.
    pub(in crate::basis::nest) fn begin(&mut self, asid: Asid) -> Asid {
        let target = match self.scope {
            Scope::AddressSpace => asid,
            Scope::Global => Asid::kernel(),
        };
        self.invalidation = self.invalidation.begin();
        target
    }

    /// Keep all obligations available for the retry.
    pub(in crate::basis::nest) fn fail(&mut self) {
        self.invalidation = self.invalidation.fail();
    }

    /// Acknowledge publications while holding the Nest lock.
    ///
    /// # Safety
    /// The fence selected by `begin` must have completed successfully.
    pub(in crate::basis::nest) unsafe fn complete(&mut self) -> Fenced {
        for mapping in &self.mappings {
            for (_, publication) in &mut mapping.state.lock().published {
                publication.complete();
            }
        }
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
    tables: Vec<TableNode>,
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
