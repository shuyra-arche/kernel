mod page;
mod phys;
mod virt;

pub use page::{PageCount, PageIndex, PageOffset};
pub use phys::PhysAddr;
pub use virt::VirtAddr;

/// Fixed 4 KiB coordinate used by the current call ABI, Cell positions and ranges.
/// This is not the size of a physically contiguous Cell or an MMU leaf.
pub const BASE_PAGE_SHIFT: usize = 12;
pub const BASE_PAGE_SIZE: usize = 1 << BASE_PAGE_SHIFT;

/// Existing public spellings remain aliases of the ABI coordinate, not a
/// promise that every physical allocation or hardware mapping is 4 KiB.
pub const PAGE_SIZE: usize = BASE_PAGE_SIZE;
pub const PAGE_SHIFT: usize = BASE_PAGE_SHIFT;
