use crate::basis::nest::SpaceKind;
use kernel_api::address::VirtAddr;

mod kind;
pub(crate) use kind::FrameKind;

#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub pc: VirtAddr,
    pub sp: VirtAddr,
    pub fp: Option<VirtAddr>,
    pub space: SpaceKind,
}
