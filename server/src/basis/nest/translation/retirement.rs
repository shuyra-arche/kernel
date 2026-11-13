//! Resources held across CPU translation invalidation.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{Invalidation, tickets::Tickets};
use crate::metal::generic::mmu::fence::{Completed, Request, Scope};
use crate::metal::mmu::Table;

use super::super::{Asid, Span};
use crate::basis::nest::memo::publication::Publication;

/// PTE edits and their obligations stay together until the covering fence completes.
/// Address tickets become claimable in the same transition as the covering fence.
pub(in crate::basis::nest) struct Retirement {
    invalidation: Invalidation,
    scope: Scope,
    sequence: crate::basis::name::Generation,
    pending: Option<Request<Asid>>,
    pub(in crate::basis::nest) tickets: Tickets,
    pub(in crate::basis::nest) tables: Vec<Table>,
    pub(in crate::basis::nest) cells: Vec<Publication>,
    pub(in crate::basis::nest) mappings: Vec<Arc<Span>>,
}

impl Retirement {
    pub(in crate::basis::nest) fn new() -> Self {
        Self {
            invalidation: Invalidation::Clean,
            scope: Scope::AddressSpace,
            sequence: crate::basis::name::Generation::first(),
            pending: None,
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
    pub(in crate::basis::nest) fn begin(
        &mut self,
        domain: usize,
        asid: Asid,
    ) -> Result<Request<Asid>, super::super::NestError> {
        if !matches!(
            self.invalidation,
            Invalidation::Dirty | Invalidation::Failed
        ) {
            return Err(super::super::NestError::Busy);
        }
        let next = self
            .sequence
            .next()
            .ok_or(super::super::NestError::OutOfMemory)?;
        let request = Request {
            domain,
            attempt: next.get(),
            identifier: asid,
            scope: self.scope,
        };
        self.sequence = next;
        self.pending = Some(request);
        self.invalidation = self.invalidation.begin();
        Ok(request)
    }

    /// Keep all obligations available for the retry.
    pub(in crate::basis::nest) fn fail(&mut self) {
        if self.invalidation != Invalidation::Failed {
            for mapping in &self.mappings {
                mapping.changed();
            }
        }
        self.invalidation = self.invalidation.fail();
    }

    /// Acknowledge publications while holding the Nest lock.
    pub(in crate::basis::nest) fn complete(
        &mut self,
        proof: Completed<Request<Asid>>,
        memos: &mut super::super::memo::Memos,
    ) -> Result<Fenced, super::super::NestError> {
        if !self.invalidation.is_fencing()
            || self
                .pending
                .is_none_or(|expected| !proof.confirms(expected))
        {
            self.fail();
            return Err(super::super::NestError::Shootdown);
        }
        self.pending = None;
        for mapping in &self.mappings {
            memos.complete(mapping);
            mapping.changed();
        }
        self.tickets.settle();
        self.invalidation = self.invalidation.complete();
        self.scope = Scope::AddressSpace;
        Ok(Fenced {
            tables: core::mem::take(&mut self.tables),
            cells: core::mem::take(&mut self.cells),
            mappings: core::mem::take(&mut self.mappings),
        })
    }
}

/// Resources from a completed fence. Release them outside the Nest lock.
pub(in crate::basis::nest) struct Fenced {
    tables: Vec<Table>,
    cells: Vec<Publication>,
    mappings: Vec<Arc<Span>>,
}

impl Fenced {
    pub(in crate::basis::nest) fn release(self, nest: &super::super::Nest) {
        drop(self.tables);
        for retained in self.cells {
            // SAFETY: Retirement constructed this batch only after the fence
            // covering the removed PTEs completed.
            unsafe { retained.settle() };
        }
        for mapping in &self.mappings {
            let mut cursor = None;
            loop {
                let Some(page) = nest.state.lock().memos.next_page(mapping, cursor) else {
                    break;
                };
                cursor = Some(page);
                nest.finish_memo(mapping, page);
            }
            loop {
                let memo = nest.state.lock().memos.take_deleted(mapping);
                if memo.is_none() {
                    break;
                }
                drop(memo);
            }
        }
        drop(self.mappings);
    }
}
