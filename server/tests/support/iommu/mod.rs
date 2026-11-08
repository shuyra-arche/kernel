//! Test-only assembly of production portable modules; no substitute implementation.
#[path = "../../../src/machine/iommu/error.rs"]
pub(crate) mod error;
#[path = "../../../src/machine/iommu/hardware.rs"]
pub(crate) mod hardware;
#[path = "../../../src/machine/iommu/identity.rs"]
pub(crate) mod identity;
#[path = "../../../src/machine/iommu/retirement.rs"]
pub(crate) mod retirement;
pub(crate) mod riscv;
#[path = "../../../src/machine/iommu/space/mod.rs"]
pub(crate) mod space;
pub(crate) use error::{Error, HardwareError};
pub(crate) use hardware::Hardware;
pub(crate) use identity::{BatchId, BindingId};
#[cfg(test)]
pub(crate) use space::Phase;
pub(crate) use space::{IoSpace, Life, Projection};
