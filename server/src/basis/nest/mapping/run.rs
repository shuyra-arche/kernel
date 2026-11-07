use crate::basis::nest::mapping::Access;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;

/// 一段连续帧：虚拟连续、物理连续，且落在同一个当前权限相同的区间内。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellRun {
    pub(crate) start: VirtAddr,
    pub(crate) physical: PhysAddr,
    pub(crate) size: usize,
    pub(crate) current: Access,
}
