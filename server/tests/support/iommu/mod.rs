//! Test-only assembly of production portable modules; no substitute implementation.
#[path = "../../../src/metal/generic/iommu.rs"]
pub(crate) mod contract;
#[path = "../../../src/basis/nest/memo/publication/error.rs"]
pub(crate) mod error;
#[path = "../../../src/basis/nest/memo/publication/identity.rs"]
pub(crate) mod identity;
mod riscv;
pub(crate) use contract::{BatchId, Fault, Hardware, HardwareError};
pub(crate) use error::Error;
pub(crate) use riscv::*;

pub(crate) type Projection = contract::Projection<crate::basis::cell::page::PageId>;
