//! Test-only assembly of production portable modules; no substitute implementation.
#[path = "../../../src/machine/iommu/binding.rs"]
pub(crate) mod binding;
#[path = "../../../src/machine/iommu/error.rs"]
pub(crate) mod error;
#[path = "../../../src/machine/iommu/hardware.rs"]
pub(crate) mod hardware;
#[path = "../../../src/machine/iommu/identity.rs"]
pub(crate) mod identity;
#[path = "../../../src/machine/iommu/phase.rs"]
pub(crate) mod phase;
#[path = "../../../src/machine/iommu/projection.rs"]
pub(crate) mod projection;
#[path = "../../../src/machine/iommu/retirement.rs"]
pub(crate) mod retirement;
pub(crate) mod riscv;
#[path = "../../../src/machine/iommu/space.rs"]
pub(crate) mod space;
use binding::Binding;
pub(crate) use error::{Error, HardwareError};
pub(crate) use hardware::Hardware;
pub(crate) use identity::{BatchId, BindingId};
use phase::Event;
pub(crate) use phase::{Life, Phase};
pub(crate) use projection::Projection;
pub(crate) use space::IoSpace;
