//! Resources held across CPU translation invalidation.

use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{Invalidation, tickets::Tickets};
use crate::metal::generic::mmu::internal::fence::{Completed, Request, Scope};
use crate::metal::mmu::internal::Table;

use super::super::{Asid, Span};
use crate::basis::nest::memo::publication::Publication;

/// PTE edits and their obligations stay together until the covering fence completes.
/// Address tickets become claimable in the same transition as the covering fence.
pub(in crate::basis::nest) struct Retirement {
    invalidation: Invalidation,
    scope: Scope,
    sequence: crate::basis::name::Generation,
    pending: Option<Request<Asid>>,
    deferred: Option<crate::metal::mmu::internal::Deferred>,
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
            deferred: None,
            tickets: Tickets::new(),
            tables: Vec::new(),
            cells: Vec::new(),
            mappings: Vec::new(),
        }
    }

    pub(in crate::basis::nest) fn deferred(
        &self,
    ) -> Option<&crate::metal::mmu::internal::Deferred> {
        self.deferred.as_ref()
    }
    pub(in crate::basis::nest) fn park(
        &mut self,
        round: crate::metal::mmu::internal::Deferred,
    ) -> Result<(), super::super::NestError> {
        if !self.invalidation.is_fencing()
            || self.deferred.is_some()
            || self
                .pending
                .is_none_or(|expected| !round.confirms(expected))
        {
            return Err(super::super::NestError::Shootdown);
        }
        self.deferred = Some(round);
        Ok(())
    }

    pub(in crate::basis::nest) fn invalidation(&self) -> Invalidation {
        self.invalidation
    }

    /// Only this batch's retained mappings still depend on its completion.
    pub(in crate::basis::nest) fn pending_for(&self, mapping: &Arc<Span>) -> bool {
        self.mappings.iter().any(|held| Arc::ptr_eq(held, mapping))
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
        self.deferred = None;
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
    ) -> Result<Fenced, super::super::NestError> {
        if !self.invalidation.is_fencing()
            || self
                .pending
                .is_none_or(|expected| !proof.confirms(expected))
        {
            self.fail();
            return Err(super::super::NestError::Shootdown);
        }
        // Allocate before consuming any obligation. Failure leaves the entire
        // batch retained and retryable, even though the hardware fence ran.
        let mut settled = Vec::new();
        let count = self
            .mappings
            .iter()
            .map(|mapping| mapping.state.lock().memos.awaiting_cpu())
            .sum();
        if settled.try_reserve(count).is_err() {
            self.fail();
            return Err(super::super::NestError::OutOfMemory);
        }
        self.pending = None;
        self.deferred = None;
        for mapping in &self.mappings {
            {
                let mut state = mapping.state.lock();
                state.memos.complete(&mut settled);
            }
            mapping.changed();
        }
        self.tickets.settle();
        self.invalidation = self.invalidation.complete();
        self.scope = Scope::AddressSpace;
        Ok(Fenced {
            settled,
            tables: core::mem::take(&mut self.tables),
            cells: core::mem::take(&mut self.cells),
            mappings: core::mem::take(&mut self.mappings),
        })
    }
}

/// Resources from a completed fence. Release them outside the Nest lock.
pub(in crate::basis::nest) struct Fenced {
    settled: Vec<super::super::memo::Settled>,
    tables: Vec<Table>,
    cells: Vec<Publication>,
    mappings: Vec<Arc<Span>>,
}

impl Fenced {
    pub(in crate::basis::nest) fn release(self) {
        drop(self.tables);
        for retained in self.cells {
            // SAFETY: Retirement constructed this batch only after the fence
            // covering the removed PTEs completed.
            unsafe { retained.settle() };
        }
        for settled in self.settled {
            settled.release();
        }
        for mapping in &self.mappings {
            loop {
                let memo = {
                    let mut state = mapping.state.lock();
                    state.take_deleted(mapping.initial_start())
                };
                if memo.is_none() {
                    break;
                }
                drop(memo);
            }
        }
        drop(self.mappings);
    }
}
