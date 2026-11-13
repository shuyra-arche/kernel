//! Persistent, branch-owned Flat window blocks. Source publication and transfer
//! admission consult this same record; Page gates cover aliases after movement.
use super::epoch::Epoch as NativeBlock;
use super::maintenance::{MaintenanceUse, Stage};
use super::*;
use crate::basis::cell::page::{Block, Page};
use crate::basis::unit::task::Task;

pub(crate) struct RangeBlock {
    authority: Arc<Pend>,
    geometry: Arc<Geometry>,
    pages: SpinLock<Vec<Block>>,
    native: SpinLock<Option<NativeBlock>>,
    pub(super) uses: SpinLock<Vec<Weak<MaintenanceUse>>>,
    pub(super) stages: SpinLock<Vec<Weak<Stage>>>,
}

impl RangeBlock {
    /// Immutable upper bound of the originating branch. Takeover changes the
    /// live owner, never the data rights carried by staged Pages.
    pub(crate) fn staged_access(&self, access: Access) -> bool {
        self.native.lock().as_ref().is_some_and(|r| {
            r.associate && (!access.contains(Access::WRITE) || r.data == Modifiable::Yes)
        })
    }
    pub(crate) fn recover(
        &self,
        expected: kernel_api::model::flat::BlockRef,
        actor: &Arc<Task>,
        token: PendId,
        authority: &Arc<Pend>,
    ) -> Result<(), PendFail> {
        if !self.settled() || self.has_uses() {
            return Err(PendFail::Busy);
        }
        self.native
            .lock()
            .as_mut()
            .ok_or(PendFail::Denied)?
            .takeover(expected, actor, token, authority, self.window())
    }
    pub(crate) fn retained(&self, page: &Arc<Page>) -> bool {
        self.pages
            .lock()
            .iter()
            .any(|b| Arc::ptr_eq(b.page(), page))
    }
    /// Rollback only a new gate prepared by an atomic holding transfer under COMMIT.
    pub(crate) fn cancel_page(&self, page: &Arc<Page>) {
        self.pages.lock().retain(|b| !Arc::ptr_eq(b.page(), page));
    }
    pub(crate) fn owner(&self) -> Arc<Pend> {
        self.native
            .lock()
            .as_ref()
            .map_or_else(|| self.authority.clone(), |record| record.authority.clone())
    }
    pub(crate) fn owns(&self, authority: &Pend) -> bool {
        core::ptr::eq(self.owner().as_ref(), authority)
    }
    pub(crate) fn owner_live(&self) -> bool {
        self.native
            .lock()
            .as_ref()
            .map_or_else(|| self.authority.alive(), |r| r.live(self.window()))
    }
    pub(crate) fn owner_entry(
        &self,
    ) -> Result<(Arc<Task>, Arc<crate::basis::pend::PendEntry>), PendFail> {
        let record = self.native.lock();
        let r = record.as_ref().ok_or(PendFail::Denied)?;
        let task = r
            .task
            .upgrade()
            .filter(|task| crate::basis::pend::live(task))
            .ok_or(PendFail::Dead)?;
        let entry = crate::basis::pend::locate(&task, r.token).ok_or(PendFail::Denied)?;
        if !r.live(self.window())
            || !Arc::ptr_eq(entry.authority(), &r.authority)
            || !entry.permits(kernel_api::model::pend::Operations::Flat(
                kernel_api::model::pend::FlatRights::BLOCK,
            ))
        {
            return Err(PendFail::Denied);
        }
        Ok((task, entry))
    }
    pub(crate) fn covers(&self, range: &FlatRange) -> bool {
        let native = self.native.lock();
        let Some(r) = native.as_ref() else {
            return false;
        };
        let (offset, count) = self.window();
        range.source_id() == r.reference.flat()
            && range.index().get() >= r.base + offset.get()
            && range
                .index()
                .get()
                .checked_add(range.page_count().get())
                .is_some_and(|end| end <= r.base + offset.get() + count.get())
    }
    pub(crate) fn permit(
        &self,
        page: &Arc<Page>,
    ) -> Result<crate::basis::cell::page::Maintenance, PendFail> {
        self.pages
            .lock()
            .iter()
            .find(|b| Arc::ptr_eq(b.page(), page))
            .ok_or(PendFail::Denied)?
            .permit()
            .map_err(|_| PendFail::Busy)
    }
    pub(crate) fn reference(&self) -> kernel_api::model::flat::BlockRef {
        self.native
            .lock()
            .as_ref()
            .expect("native maintenance")
            .reference
    }

    pub(crate) fn native(&self) -> bool {
        self.native.lock().is_some()
    }

    pub(crate) fn phase(&self) -> kernel_api::model::flat::BlockPhase {
        use kernel_api::model::flat::BlockPhase;
        if !self.owner_live() {
            return BlockPhase::Orphaned;
        }
        self.native
            .lock()
            .as_ref()
            .map_or(BlockPhase::Draining, |record| record.phase)
    }

    pub(crate) fn set_phase(&self, phase: kernel_api::model::flat::BlockPhase) {
        self.native
            .lock()
            .as_mut()
            .expect("native maintenance")
            .phase = phase;
    }
    pub(crate) fn inspect(&self) -> kernel_api::model::flat::BlockStatus {
        let maintenance = self
            .pages
            .lock()
            .iter()
            .map(|b| b.page().maintenance_obligations())
            .sum::<u64>();
        let failed = self.pages.lock().iter().any(|b| b.page().quarantined()) || self.failed_uses();
        let record = self.native.lock();
        let record = record.as_ref().expect("native maintenance");
        let (offset, count) = self.window();
        kernel_api::model::flat::BlockStatus {
            block: record.reference,
            actor: record.actor,
            phase: if record.live(self.window()) {
                record.phase
            } else {
                kernel_api::model::flat::BlockPhase::Orphaned
            },
            offset: offset.get(),
            count: count.get(),
            ordinary: self
                .pages
                .lock()
                .iter()
                .map(|block| block.page().obligations())
                .sum::<u64>()
                .saturating_sub(maintenance),
            maintenance,
            failed,
        }
    }

    pub(crate) fn matches(
        &self,
        reference: kernel_api::model::flat::BlockRef,
        actor: TaskId,
        authority: &Arc<Pend>,
    ) -> bool {
        self.native.lock().as_ref().is_some_and(|record| {
            record.reference == reference
                && record.actor == actor
                && Arc::ptr_eq(&record.authority, authority)
        })
    }

    pub(crate) fn vacate(
        &self,
        range: &crate::basis::flat::FlatRange,
        expected: kernel_api::model::flat::SlotVersion,
    ) -> Result<kernel_api::model::flat::SlotVersion, crate::basis::flat::residency::Error> {
        range.vacate_held(expected, &mut self.pages.lock())
    }
    pub(crate) fn window(&self) -> (PageOffset, PageCount) {
        self.geometry.window()
    }
    pub(crate) fn retain(&self, page: Arc<Page>) -> Result<(), PendFail> {
        let mut pages = self.pages.lock();
        if pages.iter().any(|held| Arc::ptr_eq(held.page(), &page)) {
            return Ok(());
        }
        pages.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        let change = crate::basis::cell::change::acquire(&page).map_err(|_| PendFail::Busy)?;
        pages.push(Block::new(change).map_err(|_| PendFail::Busy)?);
        Ok(())
    }
    pub(crate) fn settled(&self) -> bool {
        self.pages.lock().iter().all(Block::settled)
    }
}
impl SourceAuthorization {
    pub(crate) fn inspect_block(
        &self,
        block: &RangeBlock,
    ) -> Result<kernel_api::model::flat::BlockStatus, PendFail> {
        let mut status = block.inspect();
        for binding in self.bindings()? {
            let Some(nest) = binding.nest().upgrade() else {
                continue;
            };
            let Some(source) = nest.binding_source(&binding) else {
                continue;
            };
            let (offset, count) = source.window();
            if core::ptr::eq(source.source().as_ref(), self)
                && block.geometry.overlaps(offset, count)
                && nest.binding_failed(&binding)
            {
                status.failed = true;
                break;
            }
        }
        Ok(status)
    }
    pub(crate) fn block_ref(
        &self,
        reference: kernel_api::model::flat::BlockRef,
    ) -> Option<Arc<RangeBlock>> {
        self.blocks
            .lock()
            .iter()
            .find(|b| b.native() && b.reference() == reference)
            .cloned()
    }
    /// Discover staged historical Pages through existing records; never infer a
    /// physical identity from the now-Missing original Slot.
    pub(crate) fn retain_staged(&self, target: &Arc<RangeBlock>) -> Result<(), PendFail> {
        let blocks = {
            let known = self.blocks.lock();
            let mut blocks = Vec::new();
            blocks
                .try_reserve(known.len())
                .map_err(|_| PendFail::OutOfMemory)?;
            blocks.extend(known.iter().filter(|b| !Arc::ptr_eq(b, target)).cloned());
            blocks
        };
        let base = self.full_range().ok_or(PendFail::Dead)?.index().get();
        let (offset, count) = target.window();
        for block in blocks {
            let pages = {
                let stages = block.stages.lock();
                let held = block.pages.lock();
                let mut pages = Vec::new();
                pages
                    .try_reserve(stages.len())
                    .map_err(|_| PendFail::OutOfMemory)?;
                for stage in stages.iter().filter_map(Weak::upgrade) {
                    if stage.origin >= base + offset.get()
                        && stage.origin < base + offset.get() + count.get()
                    {
                        if let Some(page) = held.iter().find(|b| b.page().id() == stage.page) {
                            pages.push(page.page().clone());
                        }
                    }
                }
                pages
            };
            for page in pages {
                target.retain(page.clone())?;
                block.retire_uses(Some(page.id()))?;
            }
        }
        Ok(())
    }
    /// All fallible identity preparation precedes closing source admission.
    /// Caller holds COMMIT until the accepted result is recorded.
    pub(crate) fn native_block(
        &self,
        authority: &Arc<Pend>,
        offset: PageOffset,
        count: PageCount,
        flat: kernel_api::model::flat::FlatId,
        actor: &Arc<Task>,
        token: PendId,
    ) -> Result<Arc<RangeBlock>, PendFail> {
        use kernel_api::model::flat::{BlockGeneration, BlockPhase, BlockRef};
        if let Some(block) = self.blocking(authority) {
            if block.window() != (offset, count)
                || !block.native()
                || !block.matches(block.reference(), actor.ident.id, authority)
                || !matches!(block.phase(), BlockPhase::Draining | BlockPhase::Held)
            {
                return Err(PendFail::Denied);
            }
            return Ok(block);
        }
        static IDS: crate::basis::name::Serial =
            crate::basis::name::Serial::new(1, isize::MAX as usize);
        let reference = BlockRef::new(
            flat,
            IDS.allocate().ok_or(PendFail::OutOfMemory)? as u64,
            BlockGeneration::INITIAL,
        )
        .expect("block identity");
        let block = self.block(authority, offset, count)?;
        *block.native.lock() = Some(NativeBlock::new(
            reference,
            actor,
            token,
            authority,
            self.full_range().expect("validated source").index().get(),
        ));
        Ok(block)
    }
    pub(crate) fn block(
        &self,
        authority: &Arc<Pend>,
        offset: PageOffset,
        count: PageCount,
    ) -> Result<Arc<RangeBlock>, PendFail> {
        let _commit = crate::basis::commit();
        let gate = self.revoked.lock();
        if !authority.alive() || *gate {
            return Err(PendFail::Denied);
        }
        self.full_range()
            .and_then(|range| range.range(offset, count).ok())
            .ok_or(PendFail::NotAligned)?;
        let mut blocks = self.blocks.lock();
        if let Some(block) = blocks.iter().find(|b| b.owns(authority)) {
            return Ok(block.clone());
        }
        if blocks.len() >= 1024 {
            return Err(PendFail::OutOfMemory);
        }
        blocks.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        let block = Arc::try_new(RangeBlock {
            authority: authority.clone(),
            geometry: Geometry::new(offset, count)?,
            pages: SpinLock::new(Vec::new()),
            native: SpinLock::new(None),
            uses: SpinLock::new(Vec::new()),
            stages: SpinLock::new(Vec::new()),
        })
        .map_err(|_| PendFail::OutOfMemory)?;
        blocks.push(block.clone());
        Ok(block)
    }
    /// An owner may move a new Page into a blocked window only after enrolling
    /// its physical gate and settling any old hardware users. Failure retains
    /// that gate and leaves the transfer transaction untouched.
    pub(crate) fn retain_blocked(
        &self,
        offset: PageOffset,
        count: PageCount,
        page: &Arc<Page>,
    ) -> Result<(), PendFail> {
        let blocks = self.blocks.lock();
        for block in blocks.iter().filter(|b| b.geometry.overlaps(offset, count)) {
            block.retain(page.clone())?;
            if !block.settled() {
                return Err(PendFail::Busy);
            }
        }
        Ok(())
    }
    pub(crate) fn moving(&self) -> bool {
        !self.inflight.lock().is_empty()
    }
    pub(crate) fn blocking(&self, authority: &Arc<Pend>) -> Option<Arc<RangeBlock>> {
        self.blocks
            .lock()
            .iter()
            .find(|b| b.owns(authority))
            .cloned()
    }
    pub(crate) fn blocked_by(&self, authority: &Arc<Pend>) -> bool {
        self.blocks.lock().iter().any(|b| b.owns(authority))
    }
    pub(crate) fn allow(&self, authority: &Arc<Pend>) {
        // Drop Page gates after releasing the source registry lock.
        let removed = {
            let mut blocks = self.blocks.lock();
            blocks
                .iter()
                .position(|b| b.owns(authority))
                .map(|at| blocks.remove(at))
        };
        drop(removed);
    }
    pub(crate) fn check_blocks(
        &self,
        offset: PageOffset,
        count: PageCount,
        owner: Option<&Pend>,
    ) -> Result<(), PendFail> {
        if self
            .blocks
            .lock()
            .iter()
            .any(|b| b.geometry.overlaps(offset, count) && owner.is_none_or(|owner| !b.owns(owner)))
        {
            return Err(PendFail::Busy);
        }
        Ok(())
    }
    pub(crate) fn check_move(
        &self,
        offset: PageOffset,
        count: PageCount,
        owner: Option<&Pend>,
    ) -> Result<(), PendFail> {
        if self.revoked() {
            return Err(PendFail::Denied);
        }
        self.check_blocks(offset, count, owner)?;
        self.check_unblocked_window(offset, count)
    }
}
