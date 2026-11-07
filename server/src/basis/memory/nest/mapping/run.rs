use crate::basis::memory::address::{PhysAddr, VirtAddr};
use crate::basis::memory::nest::mapping::Access;

/// 一段连续帧：虚拟连续、物理连续，且落在同一个当前权限相同的区间内。
#[derive(Debug, Clone, Copy)]
pub(in crate::basis::memory::nest) struct CellRun {
    pub(in crate::basis::memory::nest) start: VirtAddr,
    pub(in crate::basis::memory::nest) physical: PhysAddr,
    pub(in crate::basis::memory::nest) size: usize,
    pub(in crate::basis::memory::nest) current: Access,
}
