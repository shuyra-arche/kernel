//! RISC-V CQ 编码、完成读数与受信任寄存器接口驱动的命令引擎。
//! 常规启动保持 DDTP Off；内部单设备 Domain 可发布 Sv39 投影。
//! 翻译撤销与平台结清分开，不把 CQ/FQ 读数作为完整 Hardware 证明。
//! 依据 RISC-V IOMMU v1.0.1 §4.1 / §5。

pub(crate) mod adapter;
pub(crate) mod controllers;
pub(crate) mod domain;
mod engine;
pub(crate) mod topology;

pub(crate) use engine::{Engine, EngineError, Fault, Phase, fence_complete, registers};

pub(crate) mod interrupt;
pub(crate) mod mmio;
pub(crate) mod pci;
pub(crate) mod retirement;
