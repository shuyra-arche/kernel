#[path = "../../../src/metal/riscv/iommu/adapter.rs"]
pub(crate) mod adapter;
#[path = "controllers.rs"]
pub(crate) mod controllers;
#[path = "../../../src/metal/riscv/iommu/domain/mod.rs"]
pub(crate) mod domain;
#[path = "../../../src/metal/riscv/iommu/engine/mod.rs"]
pub(crate) mod engine;
#[cfg(test)]
pub(crate) use engine::Command;
pub(crate) use engine::{Engine, EngineError, Phase, fence_complete, registers};

#[cfg(test)]
#[path = "../../iommu/domain.rs"]
mod transitions;
