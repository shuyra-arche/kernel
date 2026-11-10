//! RISC-V CQ 编码、完成读数与受信任寄存器接口驱动的命令引擎。
//! 常规启动保持 DDTP Off；内部单设备 Domain 可发布 Sv39 投影。
//! 翻译撤销与平台结清分开，不把 CQ/FQ 读数作为完整 Hardware 证明。
//! 依据 RISC-V IOMMU v1.0.1 §4.1 / §5。

pub(crate) mod adapter;
pub(crate) mod controllers;
pub(crate) mod domain;
mod engine;
mod source;

pub(crate) use engine::{Engine, EngineError, Phase, fence_complete, registers};
pub(crate) use source::bind;

pub(crate) mod mmio;

use crate::basis::cell::page::PageId;
pub(crate) use crate::metal::generic::iommu::{BatchId, Fault, HardwareError};
pub(crate) type Projection = crate::metal::generic::iommu::Projection<PageId>;
pub(crate) type Backend = crate::metal::generic::iommu::Backend<PageId>;
