//! References to one native maintenance epoch. The range record owns admission;
//! Memo owns each use and its hardware receipt. Weak lists only discover cleanup.
use super::{RangeBlock, SourceAuthorization};
use crate::basis::{
    cell::page::{Maintenance, Page, PageId},
    nest::{Access, Nest, Span, memo::source::Source},
    pend::Pend,
    unit::task::Task,
};
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use kernel_api::{
    abi::call::{cell::CellFail, pend::PendFail},
    model::{
        flat::{BlockPhase, BlockRef},
        modifiable::Modifiable,
        pend::{FlatRights, Operations},
    },
};

pub(crate) struct Stage {
    pub(crate) block: Arc<RangeBlock>,
    pub(crate) page: PageId,
    pub(crate) origin: usize,
}

pub(crate) struct MaintenanceUse {
    block: Arc<RangeBlock>,
    reference: BlockRef,
    origin: Arc<SourceAuthorization>,
    permit: Maintenance,
    nest: Weak<Nest>,
    span: Weak<Span>,
    index: usize,
    generation: u64,
    staged: bool,
}
impl MaintenanceUse {
    /// Capture the epoch under COMMIT. Publication rechecks this prepared use
    /// under COMMIT before installing it in Memo and touching the backend.
    pub(crate) fn prepare(
        block: Arc<RangeBlock>,
        actor: &Arc<Task>,
        source: &Source,
        page: &Arc<Page>,
        nest: &Arc<Nest>,
        span: &Arc<Span>,
        index: usize,
        generation: u64,
        access: Access,
    ) -> Result<Arc<Self>, CellFail> {
        let (owner, entry) = block.owner_entry().map_err(failure)?;
        if !Arc::ptr_eq(&owner, actor)
            || (!nest.is_external() && !Arc::ptr_eq(nest, &actor.ident.team.nest))
        {
            return Err(CellFail::Denied);
        }
        let origin = entry
            .target()
            .memory()
            .and_then(|m| m.authorization())
            .ok_or(CellFail::Denied)?;
        let range = source.check(access)?;
        let staged = !block.covers(&range);
        if staged && !range.staged_by(&block, page.id()) {
            return Err(CellFail::Denied);
        }
        let permit = block.permit(page).map_err(failure)?;
        let usage = Arc::try_new(Self {
            reference: block.reference(),
            block: block.clone(),
            origin,
            permit,
            nest: Arc::downgrade(nest),
            span: Arc::downgrade(span),
            index,
            generation,
            staged,
        })
        .map_err(|_| CellFail::OutOfMemory)?;
        usage.check(source, access)?;
        let mut uses = block.uses.lock();
        uses.retain(|u| u.strong_count() != 0);
        if uses.len() >= 1024 {
            return Err(CellFail::OutOfMemory);
        }
        uses.try_reserve(1).map_err(|_| CellFail::OutOfMemory)?;
        uses.push(Arc::downgrade(&usage));
        Ok(usage)
    }
    pub(crate) fn permit(&self) -> &Maintenance {
        &self.permit
    }
    pub(crate) fn owned_by(&self, actor: &Arc<Task>) -> bool {
        self.block
            .owner_entry()
            .is_ok_and(|(owner, _)| Arc::ptr_eq(&owner, actor))
            && self.block.reference() == self.reference
    }
    pub(crate) fn source_owner(&self, source: &Source) -> Option<Arc<Pend>> {
        Arc::ptr_eq(source.source(), &self.origin).then(|| self.block.owner())
    }
    pub(crate) fn check(&self, source: &Source, access: Access) -> Result<(), CellFail> {
        self.check_retained(source, access)?;
        let range = source.check(access)?;
        if self.staged {
            if !range.staged_by(&self.block, self.permit.change().page().id()) {
                return Err(CellFail::Denied);
            }
        } else if !Arc::ptr_eq(source.source(), &self.origin) || !self.block.covers(&range) {
            return Err(CellFail::Denied);
        }
        let (offset, count) = self.block.window();
        self.origin
            .check_move(offset, count, Some(&self.block.owner()))
            .map_err(failure)?;
        source.same_page(self.permit.change().page())?;
        Ok(())
    }
    /// Recheck a registered publication without treating its own External loan
    /// as revoked data authority. The retained receipt prevents holding moves.
    pub(crate) fn check_retained(&self, source: &Source, access: Access) -> Result<(), CellFail> {
        let (_, entry) = self.block.owner_entry().map_err(failure)?;
        if self.block.reference() != self.reference
            || self.block.phase() != BlockPhase::Held
            || source.retiring()
        {
            return Err(CellFail::Denied);
        }
        if self.staged {
            if !self.block.staged_access(access)
                || !entry.permits(Operations::Flat(FlatRights::ASSOCIATE))
                || access.contains(Access::WRITE) && entry.modifiable() != Modifiable::Yes
                || !self.origin.ceiling().contains(access)
            {
                return Err(CellFail::Denied);
            }
        }
        let (offset, count) = self.block.window();
        self.origin
            .check_blocks(offset, count, Some(entry.authority()))
            .map_err(failure)?;
        let owner = self.source_owner(source);
        let (offset, count) = source.window();
        source
            .source()
            .check_blocks(offset, count, owner.as_deref())
            .map_err(failure)?;
        self.block
            .permit(self.permit.change().page())
            .map_err(failure)?;
        Ok(())
    }
    fn retire(self: &Arc<Self>) -> Result<(), PendFail> {
        let nest = self.nest.upgrade().ok_or(PendFail::Busy)?;
        let span = self.span.upgrade().ok_or(PendFail::Busy)?;
        nest.retire_maintenance(&span, self.index, self.generation, self)
    }
}
impl RangeBlock {
    pub(super) fn failed_uses(&self) -> bool {
        // Do not hold the discovery list while inspecting a Nest or dropping
        // its final strong reference. No allocation is needed for observation.
        let mut index = 0;
        loop {
            let usage = self.uses.lock().get(index).cloned();
            let Some(usage) = usage else { return false };
            index += 1;
            if let Some(usage) = usage.upgrade() {
                if let (Some(nest), Some(span)) = (usage.nest.upgrade(), usage.span.upgrade()) {
                    if nest.maintenance_failed(&span, usage.index, &usage) {
                        return true;
                    }
                }
            }
        }
    }
    pub(crate) fn has_uses(&self) -> bool {
        self.uses.lock().iter().any(|u| u.strong_count() != 0)
    }
    pub(crate) fn stage(
        self: &Arc<Self>,
        page: PageId,
        origin: usize,
    ) -> Result<Arc<Stage>, PendFail> {
        let mut stages = self.stages.lock();
        stages.retain(|s| s.strong_count() != 0);
        if stages.len() >= 1024 {
            return Err(PendFail::OutOfMemory);
        }
        stages.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        let stage = Arc::try_new(Stage {
            block: self.clone(),
            page,
            origin,
        })
        .map_err(|_| PendFail::OutOfMemory)?;
        stages.push(Arc::downgrade(&stage));
        Ok(stage)
    }
    pub(crate) fn staged(&self) -> bool {
        self.stages.lock().iter().any(|s| s.strong_count() != 0)
    }
    pub(crate) fn retire_uses(&self, page: Option<PageId>) -> Result<(), PendFail> {
        let uses = {
            let mut known = self.uses.lock();
            known.retain(|u| u.strong_count() != 0);
            let mut uses = Vec::new();
            uses.try_reserve(known.len())
                .map_err(|_| PendFail::OutOfMemory)?;
            uses.extend(
                known
                    .iter()
                    .filter_map(Weak::upgrade)
                    .filter(|u| page.is_none_or(|p| u.permit.change().page().id() == p)),
            );
            uses
        };
        let mut busy = false;
        for usage in uses.iter().take(4) {
            busy |= usage.retire().is_err();
        }
        if busy || uses.len() > 4 {
            Err(PendFail::Busy)
        } else {
            Ok(())
        }
    }
}
fn failure(error: PendFail) -> CellFail {
    match error {
        PendFail::Busy => CellFail::Busy,
        PendFail::OutOfMemory => CellFail::OutOfMemory,
        _ => CellFail::Denied,
    }
}
