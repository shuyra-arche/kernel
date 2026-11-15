pub(crate) mod allocator;
pub(crate) mod generic;

pub(crate) mod riscv;
// The only architecture selection point. Public mechanism paths select one
// complete provider; no mechanism-first directory forwards to another tree.
pub(crate) use riscv::{
    boot, clock, console, hart, image, interrupt, layout, memory, mmu, physical, platform, sbi,
    stop, switcher, sync,
};

#[cfg(feature = "latency")]
pub(crate) mod measure;
