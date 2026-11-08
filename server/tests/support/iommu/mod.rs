//! Test-only assembly of production portable modules; no substitute implementation.
#[path = "../../../src/metal/generic/iommu.rs"]
pub(crate) mod contract;
#[path = "../../../src/metal/iommu/error.rs"]
pub(crate) mod error;
#[path = "../../../src/metal/iommu/identity.rs"]
pub(crate) mod identity;
#[path = "../../../src/metal/iommu/retirement.rs"]
pub(crate) mod retirement;
pub(crate) mod riscv;
#[path = "../../../src/metal/iommu/space/mod.rs"]
pub(crate) mod space;
pub(crate) use contract::{BatchId, BindingId, Fault, Hardware, HardwareError};
pub(crate) use error::Error;
#[cfg(test)]
pub(crate) use space::Phase;
pub(crate) use space::{IoSpace, Life};

pub(crate) type Projection = contract::Projection<crate::basis::cell::page::PageId>;
