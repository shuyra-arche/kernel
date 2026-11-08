//! 设备地址投影的内核协议：登记、撤销批次与可信结清。
//!
//! 纯状态决策、Kernel 持有的回收记录与受独占访问保护的硬件效果。
//! 单设备适配器已由 QEMU 验证；普通启动仍无 DMA 授权 ABI 或队列中断接线。
pub(crate) mod aperture;
mod error;
mod identity;
pub(crate) mod retirement;
pub(crate) mod riscv;
pub(crate) mod runtime;

pub(crate) use crate::metal::generic::iommu::{BatchId, BindingId, Fault, Hardware, HardwareError};
pub(crate) use error::Error;

use crate::basis::cell::page::PageId;
use crate::metal::generic::iommu as contract;

pub(crate) type Projection = contract::Projection<PageId>;
pub(crate) type Backend = contract::Backend<PageId>;

mod space;
pub(crate) use space::{IoSpace, Life, Phase};
