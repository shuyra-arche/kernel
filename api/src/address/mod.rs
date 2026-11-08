mod page;
mod phys;
mod virt;

pub use page::{PageCount, PageIndex, PageOffset};
pub use phys::PhysAddr;
pub use virt::VirtAddr;

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;
