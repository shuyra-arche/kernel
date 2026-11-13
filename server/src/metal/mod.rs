pub(crate) mod allocator;
pub(crate) mod generic;

pub(crate) mod riscv;
// The only architecture selection point. Public mechanism paths select one
// complete provider; no mechanism-first directory forwards to another tree.
pub(crate) use riscv::{
    clock, console, hart, image, interrupt, iommu, layout, memory, mmu, platform, stop, switcher,
    sync,
};

#[cfg(feature = "latency")]
pub(crate) mod measure;
