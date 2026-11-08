//! 一次选址的完整数据：选址方式与起始对齐。
//! 选址值嵌入 CellCreate 的单次快照；ABI codec 负责 checked 解码。
//!
//! 描述里只有选址数据：没有来源、授权、清理，也没有第二份长度（长度只来自
//! `ReferenceRange`）。区域固定为普通用户窗口（旧 `Mmap` 同口径）。

use crate::address::VirtAddr;

use super::PAGE_SIZE;

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

/// 三者共用同一份占用检查：`Lowest` / `Highest` 在窗口内选空档，`Exact` 直接给出
/// 固定跨度——即使它落在所有窗口之外，也按同一全局占用登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementAt {
    Lowest,
    Highest,
    Exact(VirtAddr),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementError {
    Tag,
    Align,
}
