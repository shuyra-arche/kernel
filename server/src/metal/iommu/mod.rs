//! 设备地址投影的内核协议：登记、撤销批次与可信结清。
//!
//! 纯状态决策、Kernel 持有的回收记录与受独占访问保护的硬件效果。
//! 单设备适配器已由 QEMU 验证；普通启动仍无 DMA 授权 ABI 或队列中断接线。
mod fault;

pub(crate) mod aperture;
mod error;
mod hardware;
mod identity;
pub(crate) mod retirement;
pub(crate) mod runtime;

pub(crate) use error::Error;
pub(crate) use error::HardwareError;
pub(crate) use hardware::Hardware;
pub(crate) use identity::BatchId;
pub(crate) use identity::BindingId;

mod space;
pub(crate) use space::{IoSpace, Life, Phase, Projection};

pub(crate) use fault::Fault;
