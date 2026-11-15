use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::metal::allocator::InitError;
use crate::metal::sync::OnceLock;

use super::Kind;

pub(in super::super) const BLOCK_KIND_SHIFT: usize = 7;

struct BlockKinds {
    ptr: *mut Kind,
    len: usize,
    pages: crate::metal::allocator::pages::Pages<'static>,
}

unsafe impl Send for BlockKinds {}
unsafe impl Sync for BlockKinds {}

static BLOCK_KINDS: OnceLock<BlockKinds> = OnceLock::new();

pub(crate) fn install_block_kinds() -> Result<(), InitError> {
    let pages =
        crate::metal::allocator::pages::Pages::new(crate::entry::boot::facts::info().memory.free());
    let slots = pages.len() * (kernel_api::address::PAGE_SIZE >> BLOCK_KIND_SHIFT);
    let mut v: Vec<Kind> = Vec::new();
    v.try_reserve(slots).map_err(|_| InitError::OutOfMemory)?;
    v.resize(slots, Kind::Plain);
    let slice: &'static mut [Kind] = Box::leak(v.into_boxed_slice());
    let h = BlockKinds {
        ptr: slice.as_mut_ptr(),
        len: slice.len(),
        pages,
    };
    BLOCK_KINDS
        .set(h)
        .map_err(|_| InitError::AlreadyInitialized)
}

fn block_kind_table() -> (
    &'static mut [Kind],
    crate::metal::allocator::pages::Pages<'static>,
) {
    let h = BLOCK_KINDS
        .get()
        .expect("block kinds not installed (block::init 应先调 install_block_kinds)");
    let t = unsafe { core::slice::from_raw_parts_mut(h.ptr, h.len) };
    (t, h.pages)
}

pub(in super::super) fn note_block_kind(addr: usize, power: usize, k: Kind) {
    if power < BLOCK_KIND_SHIFT {
        return;
    }
    let (t, pages) = block_kind_table();
    let first = index(pages, addr);
    t[first..first + (1 << (power - BLOCK_KIND_SHIFT))].fill(k);
}

pub(in super::super) fn block_kind_at(addr: usize, power: usize) -> Kind {
    if power < BLOCK_KIND_SHIFT {
        return Kind::Plain;
    }
    let (t, pages) = block_kind_table();
    t[index(pages, addr)]
}

fn index(pages: crate::metal::allocator::pages::Pages<'_>, address: usize) -> usize {
    let page = kernel_api::address::PAGE_SIZE;
    pages.index(address).expect("block outside admitted RAM") * (page >> BLOCK_KIND_SHIFT)
        + (address % page >> BLOCK_KIND_SHIFT)
}
