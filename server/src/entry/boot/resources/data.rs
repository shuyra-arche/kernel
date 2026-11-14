//! Immutable boot bytes become managed RAM with a real read-only source root.
use crate::basis::{
    cell::{self, page::Owner},
    flat::{Flat, sharing::Sharing},
    nest::Access,
    pend::{PendEntry, SourceAuthorization},
    view::View,
};
use alloc::vec::Vec;
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PAGE_SIZE, PageCount, PageIndex},
    model::{
        identity::{Mark, TaskId},
        modifiable::Modifiable,
    },
};

/// # Safety
/// The physical range is validated immutable boot data, readable through the
/// Kernel's admitted identity map for the duration of this unpublished copy.
/// This is not an importer for caller-supplied addresses or MMIO.
pub(in crate::entry::boot) unsafe fn snapshot(
    base: usize,
    len: usize,
) -> Result<PendEntry, PendFail> {
    if base == 0 || len == 0 || len > isize::MAX as usize || base.checked_add(len).is_none() {
        return Err(PendFail::Denied);
    }
    let count = PageCount::new(len.div_ceil(PAGE_SIZE)).ok_or(PendFail::Denied)?;
    let size = count.checked_bytes().ok_or(PendFail::Denied)?;
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(count.get())
        .map_err(|_| PendFail::OutOfMemory)?;
    let address = crate::metal::mmu::canonical(base).as_usize();
    for index in 0..count.get() {
        let cell = cell::allocate(Owner::Kernel).map_err(|_| PendFail::OutOfMemory)?;
        let offset = index * PAGE_SIZE;
        let bytes = (len - offset).min(PAGE_SIZE);
        // SAFETY: input is the admitted immutable boot range. Each destination
        // is a new unpublished Cell with no alias; the allocator zeroes padding.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (address + offset) as *const u8,
                cell.as_mut_ptr(),
                bytes,
            );
        }
        cells.push((PageIndex::new(index), cell));
    }
    let range = Flat::from_cells(Owner::Kernel, count, cells)
        .map_err(|_| PendFail::OutOfMemory)?
        .whole();
    let owner = TaskId::new(0);
    let source = SourceAuthorization::new(range.clone(), Access::READ, owner)?;
    let view = Sharing::from_source(range, size, owner)?;
    let root = PendEntry::try_new(View::Share(view.clone()), Mark::NONE, Modifiable::No, None)?;
    let claim = source.claim()?;
    view.attach_source(&source);
    source
        .begin_withdraw()?
        .adopt(&view, root.token(), root.authority().clone())?;
    claim.commit();
    Ok(root)
}
