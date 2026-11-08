//! Shared contracts between Kernel mechanisms and hardware providers.
//!
//! Runtime state belongs to the consuming mechanism. Providers implement the
//! contracts using their native registers, memory formats and firmware.

pub(crate) mod clock;
pub(crate) mod interrupt;
pub(crate) mod iommu;
pub(crate) mod mmu;
pub(crate) mod trap;
