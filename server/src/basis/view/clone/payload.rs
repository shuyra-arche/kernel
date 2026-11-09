//! Immutable copied Cells held in Flat. Receiving publishes authority, not bytes.
use crate::basis::cell::page::Owner;
use crate::basis::flat::Flat;
use crate::basis::flat::FlatRange;
use crate::basis::flat::sharing::Sharing;
use crate::basis::nest::Access;
use crate::basis::nest::Nest;
use crate::basis::pend::PendEntry;
use crate::basis::pend::SourceAuthorization;
use crate::basis::view::View;
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::abi::call::clone_view::CloneFail;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;
use kernel_api::address::VirtAddr;
use kernel_api::model::cell::PAGE_SIZE;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::TaskId;
use kernel_api::model::modifiable::Modifiable;

pub(crate) struct Payload {
    source: FlatRange,
    len: usize,
}
impl Payload {
    pub(crate) fn capture(
        space: &Nest,
        address: usize,
        len: usize,
    ) -> Result<Arc<Self>, CloneFail> {
        let count = PageCount::new(len.div_ceil(PAGE_SIZE)).ok_or(CloneFail::Denied)?;
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(count.get())
            .map_err(|_| CloneFail::OutOfMemory)?;
        for index in 0..count.get() {
            // The frame allocator zeroes the whole page, including visible tail padding.
            let cell =
                crate::basis::cell::allocate(Owner::Kernel).map_err(|_| CloneFail::OutOfMemory)?;
            let offset = index * PAGE_SIZE;
            let size = (len - offset).min(PAGE_SIZE);
            // SAFETY: unpublished Page, no aliases or hardware mappings.
            let bytes = unsafe { core::slice::from_raw_parts_mut(cell.as_mut_ptr(), size) };
            if !space.copy_in(bytes, VirtAddr::wrap(address + offset)) {
                return Err(CloneFail::Denied);
            }
            cells.push((PageIndex::new(index), cell));
        }
        let source = Flat::from_cells(Owner::Kernel, count, cells)
            .map_err(|_| CloneFail::OutOfMemory)?
            .whole();
        Arc::try_new(Self { source, len }).map_err(|_| CloneFail::OutOfMemory)
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
        let view = Sharing::from_source(self.source.clone(), size, receiver)?;
        let pend = PendEntry::try_new(View::Share(view.clone()), Mark::NONE, Modifiable::No, None)?;
        view.attach_source(&source);
        source.adopt(&view, pend.token(), pend.authority().clone())?;
        source.consume();
        view.commit_source();
        Ok(pend)
    }
}
