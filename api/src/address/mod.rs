mod count;
mod index;
mod offset;
mod phys;
mod virt;

pub use count::PageCount;
pub use index::PageIndex;
pub use offset::PageOffset;
pub use phys::PhysAddr;
pub use virt::VirtAddr;

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;
