//! Persistent, branch-owned Flat window blocks. Source publication and transfer
//! admission consult this same record; Page gates cover aliases after movement.
use super::*;
use crate::basis::cell::page::{Block, Page};

pub(crate) struct RangeBlock {
    authority: Arc<Pend>,
    geometry: Arc<Geometry>,
    pages: SpinLock<Vec<Block>>,
}
impl RangeBlock {
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
        if let Some(block) = blocks.iter().find(|b| Arc::ptr_eq(&b.authority, authority)) {
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
            .find(|b| Arc::ptr_eq(&b.authority, authority))
            .cloned()
    }
    pub(crate) fn blocked_by(&self, authority: &Arc<Pend>) -> bool {
        self.blocks
            .lock()
            .iter()
            .any(|b| Arc::ptr_eq(&b.authority, authority))
    }
    pub(crate) fn allow(&self, authority: &Arc<Pend>) {
        // Drop Page gates after releasing the source registry lock.
        let removed = {
            let mut blocks = self.blocks.lock();
            blocks
                .iter()
                .position(|b| Arc::ptr_eq(&b.authority, authority))
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
        if self.blocks.lock().iter().any(|b| {
            b.geometry.overlaps(offset, count)
                && owner.is_none_or(|owner| !core::ptr::eq(owner, b.authority.as_ref()))
        }) {
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
