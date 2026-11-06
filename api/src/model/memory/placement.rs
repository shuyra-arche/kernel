//! 一次选址的完整数据：选址方式与起始对齐。//!
//! 它**不是**内联寄存器载荷：`Attach` 只过一个指向它的只读用户指针，内核
//! 先 `copy_in` 快照这 [`Placement::WORDS`] 个字，再用本文件的 codec 解码并
//! checked 校验。非法字节永远不会被当成 Rust 枚举。
//!
//! 描述里只有选址数据：没有来源、授权、清理，也没有第二份长度（长度只来自
//! `ReferenceRange`）。区域固定为普通用户窗口（旧 `Mmap` 同口径）。

use crate::model::identity::VirtAddr;

use super::PAGE_SIZE;
use super::placement_at::PlacementAt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    at: PlacementAt,
    align: usize,
}

impl Placement {
    /// 窗口内最低的满足 `align` 的空档。
    pub fn lowest(align: usize) -> Option<Self> {
        Self::checked(PlacementAt::Lowest, align)
    }

    /// 窗口内最高的空档（页对齐）。
    pub fn highest() -> Self {
        Self {
            at: PlacementAt::Highest,
            align: PAGE_SIZE,
        }
    }

    /// 定点跨度：调用方已经知道地址。
    pub fn exact(at: VirtAddr, align: usize) -> Option<Self> {
        Self::checked(PlacementAt::Exact(at), align)
    }

    pub(crate) fn checked(at: PlacementAt, align: usize) -> Option<Self> {
        if !align.is_power_of_two() || align < PAGE_SIZE {
            return None;
        }
        if let PlacementAt::Exact(address) = at
            && address.get() % align != 0
        {
            return None;
        }
        Some(Self { at, align })
    }

    pub fn at(self) -> PlacementAt {
        self.at
    }

    pub fn align(self) -> usize {
        self.align
    }
}
