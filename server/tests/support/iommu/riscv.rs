#[path = "../../../src/metal/iommu/riscv/adapter.rs"]
pub(crate) mod adapter;
#[path = "controllers.rs"]
pub(crate) mod controllers;
#[path = "../../../src/metal/iommu/riscv/domain/mod.rs"]
pub(crate) mod domain;
#[path = "../../../src/metal/iommu/riscv/engine/mod.rs"]
pub(crate) mod engine;
#[path = "../../../src/metal/iommu/riscv/topology/mod.rs"]
pub(crate) mod topology;
#[cfg(test)]
pub(crate) use engine::Command;
pub(crate) use engine::{Engine, EngineError, Phase, fence_complete, registers};

#[cfg(test)]
#[path = "../../iommu/domain.rs"]
mod transitions;
