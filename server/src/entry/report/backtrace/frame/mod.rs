use crate::basis::memory::address::VirtAddr;
use crate::basis::memory::nest::SpaceKind;

mod kind;
pub(crate) use kind::FrameKind;

#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub pc: VirtAddr,
    pub sp: VirtAddr,
    pub fp: Option<VirtAddr>,
    pub space: SpaceKind,
}
