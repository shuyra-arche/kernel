pub(crate) mod allocator;
pub(crate) mod generic;

pub(crate) mod riscv;
// The only architecture selection point. Public mechanism paths select one
// complete provider; no mechanism-first directory forwards to another tree.
pub(crate) use riscv::{
    clock, console, hart, image, interrupt, iommu, layout, mmu, stop, switcher, sync,
};
