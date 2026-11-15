#[path = "../../../src/metal/riscv/mmu/external/adapter.rs"]
pub(crate) mod adapter;
#[path = "controllers.rs"]
pub(crate) mod controllers;
#[path = "../../../src/metal/riscv/mmu/external/domain/mod.rs"]
pub(crate) mod domain;
#[path = "../../../src/metal/riscv/mmu/external/engine/mod.rs"]
pub(crate) mod engine;
#[cfg(test)]
pub(crate) use engine::Command;
pub(crate) use engine::{Engine, EngineError, Phase, fence_complete, registers};

#[cfg(test)]
#[path = "../../metal/riscv/iommu/domain.rs"]
mod transitions;
