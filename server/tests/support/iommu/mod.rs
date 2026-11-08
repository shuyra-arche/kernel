//! Test-only assembly of production portable modules; no substitute implementation.
#[path = "../../../src/metal/iommu/error.rs"]
pub(crate) mod error;
#[path = "../../../src/metal/iommu/fault.rs"]
mod fault;
#[path = "../../../src/metal/iommu/hardware.rs"]
pub(crate) mod hardware;
#[path = "../../../src/metal/iommu/identity.rs"]
pub(crate) mod identity;
#[path = "../../../src/metal/iommu/retirement.rs"]
pub(crate) mod retirement;
pub(crate) mod riscv;
#[path = "../../../src/metal/iommu/space/mod.rs"]
pub(crate) mod space;
pub(crate) use error::{Error, HardwareError};
pub(crate) use hardware::Hardware;
pub(crate) use identity::{BatchId, BindingId};
#[cfg(test)]
pub(crate) use space::Phase;
pub(crate) use space::{IoSpace, Life, Projection};

pub(crate) use fault::Fault;
