//! Immutable copied Cells. A queued Clone never aliases its sender's memory.
use crate::core::{
    memory::{address::VirtAddr, cell::Cell, flat::allocator::frame},
    work::unit::space::Space,
};
use alloc::{sync::Arc, vec::Vec};
use kernel_api::{abi::call::clone_view::CloneFail, model::memory::PAGE_SIZE};

pub(crate) struct Payload {
    cells: Vec<Cell>,
    len: usize,
}
impl Payload {
    pub(crate) fn capture(
        space: &Space,
        address: usize,
        len: usize,
    ) -> Result<Arc<Self>, CloneFail> {
        let count = len.div_ceil(PAGE_SIZE);
        let mut cells = Vec::new();
        cells.try_reserve_exact(count).map_err(|_| CloneFail::OoM)?;
        for index in 0..count {
            let cell = frame::cell(crate::core::memory::cell::Owner::Kernel)
                .map_err(|_| CloneFail::OoM)?;
            let offset = index * PAGE_SIZE;
            let size = (len - offset).min(PAGE_SIZE);
            // SAFETY: this unpublished Cell has no aliases or hardware mappings.
            let bytes = unsafe { core::slice::from_raw_parts_mut(cell.as_mut_ptr(), size) };
            if !space.copy_in(bytes, VirtAddr::wrap(address + offset)) {
                return Err(CloneFail::Denied);
            }
            cells.push(cell);
        }
        Arc::try_new(Self { cells, len }).map_err(|_| CloneFail::OoM)
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub(crate) fn copy_out(&self, space: &Space, address: usize) -> bool {
        for (index, cell) in self.cells.iter().enumerate() {
            let offset = index * PAGE_SIZE;
            let size = (self.len - offset).min(PAGE_SIZE);
            // SAFETY: payload Cells become immutable before queue publication and
            // remain owned by this Payload throughout every reader's Arc lifetime.
            let bytes = unsafe { core::slice::from_raw_parts(cell.as_mut_ptr(), size) };
            if !space.copy_out(bytes, VirtAddr::wrap(address + offset)) {
                return false;
            }
        }
        true
    }
}
