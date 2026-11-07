//! Immutable copied Cells held in Flat. Receiving publishes authority, not bytes.
use crate::basis::{
    memory::{
        address::{PageCount, PageIndex, VirtAddr},
        cell::Owner,
        flat::{Flat, FlatRange, allocator::frame},
        nest::Access,
    },
    work::{
        pend::{PendEntry, SourceAuthorization},
        unit::space::Space,
        view::{ShareView, View},
    },
};
use alloc::{sync::Arc, vec::Vec};
use kernel_api::{
    abi::call::{clone_view::CloneFail, pend::PendFail},
    model::{
        identity::{Mark, TaskId},
        memory::PAGE_SIZE,
        modifiable::Modifiable,
    },
};

pub(crate) struct Payload {
    source: FlatRange,
    len: usize,
}
impl Payload {
    pub(crate) fn capture(
        space: &Space,
        address: usize,
        len: usize,
    ) -> Result<Arc<Self>, CloneFail> {
        let count = PageCount::new(len.div_ceil(PAGE_SIZE)).ok_or(CloneFail::Denied)?;
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(count.get())
            .map_err(|_| CloneFail::OoM)?;
        for index in 0..count.get() {
            // The frame allocator zeroes the whole page, including visible tail padding.
            let cell = frame::cell(Owner::Kernel).map_err(|_| CloneFail::OoM)?;
            let offset = index * PAGE_SIZE;
            let size = (len - offset).min(PAGE_SIZE);
            // SAFETY: unpublished Cell, no aliases or hardware mappings.
            let bytes = unsafe { core::slice::from_raw_parts_mut(cell.as_mut_ptr(), size) };
            if !space.copy_in(bytes, VirtAddr::wrap(address + offset)) {
                return Err(CloneFail::Denied);
            }
            cells.push((PageIndex::new(index), cell));
        }
        let source = Flat::from_cells(Owner::Kernel, count, cells)
            .map_err(|_| CloneFail::OoM)?
            .whole();
        Arc::try_new(Self { source, len }).map_err(|_| CloneFail::OoM)
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Prepare an unpublished, independent access root. All allocations happen
    /// before consumption. Both queue and receiver temporarily hold the same Flat;
    /// no caller can access these immutable Cells until the Pend is published.
    pub(crate) fn prepare(&self, receiver: TaskId) -> Result<PendEntry, PendFail> {
        let size = self
            .source
            .page_count()
            .checked_bytes()
            .ok_or(PendFail::Denied)?;
        let source = SourceAuthorization::new(self.source.clone(), Access::READ, receiver)?;
        let view = ShareView::from_source(self.source.clone(), size, receiver)?;
        let pend = PendEntry::try_new(View::Share(view.clone()), Mark::NONE, Modifiable::No, None)?;
        view.attach_source(&source);
        source.adopt(&view, pend.token(), pend.authority().clone())?;
        source.consume();
        view.commit_source();
        Ok(pend)
    }
}
