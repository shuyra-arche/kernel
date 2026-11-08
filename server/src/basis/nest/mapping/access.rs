use alloc::vec::Vec;
use bitflags::bitflags;
use core::num::NonZeroUsize;

use crate::basis::nest::NestError;
use kernel_api::address::VirtAddr;

use crate::machine::mmu::riscv::table::PteFlags;

bitflags! {
    /// 映射权限的逻辑三位：只讲读写执行。
    ///
    /// 线上对应 PTE 的 R/W/X，位于 `CellCall` 位 1/2/3；U/G/V/A/D 属于
    /// 独立的架构属性，由挂载时的政策给出。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Access: u64 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

/// 线上三位的掩码：`CellCall` 的 flags 里只有这三位合法。
const WIRE_MASK: u64 = 0b1110;

impl Access {
    /// 线上三位折成本地三位；多一位都拒。
    ///
    /// 只做换算，不带授权口径：`Mmap` 与 `Mprotect` 各自在调用点判该拒什么。
    pub fn from_wire(flags: u64) -> Option<Self> {
        if flags & !WIRE_MASK != 0 {
            return None;
        }
        Self::from_bits(flags >> 1)
    }

    /// 页表里的 RWX 三位折成逻辑三位。
    pub fn from_pte(flags: PteFlags) -> Self {
        Self::from_bits_truncate(flags.bits() >> 1)
    }

    pub(crate) fn to_pte(self) -> PteFlags {
        PteFlags::from_bits_truncate(self.bits() << 1)
    }

    /// 合法叶子权限：RWX 非空，且写必须有读。
    pub(crate) fn is_leaf_set(self) -> bool {
        !self.is_empty() && !(self.contains(Self::WRITE) && !self.contains(Self::READ))
    }
}

/// 一段活动关系区间：`ceiling` 是权限上限，`current` 是当前逻辑权限。
#[derive(Debug, Clone, Copy)]
pub struct ActiveRange {
    pub start: VirtAddr,
    pub size: NonZeroUsize,
    pub ceiling: Access,
    pub current: Access,
}

impl ActiveRange {
    pub(crate) fn new(
        start: VirtAddr,
        size: usize,
        ceiling: Access,
        current: Access,
    ) -> Result<Self, NestError> {
        let size = NonZeroUsize::new(size).ok_or(NestError::NotAligned)?;
        if start.offset() != 0
            || !size.get().is_multiple_of(kernel_api::address::PAGE_SIZE)
            || !ceiling.contains(current)
            || !current.is_leaf_set()
        {
            return Err(NestError::NotAligned);
        }
        Ok(Self {
            start,
            size,
            ceiling,
            current,
        })
    }

    /// 含尾区间终点（饱和，不溢出）。
    pub fn end(self) -> usize {
        self.start.as_usize().saturating_add(self.size.get() - 1)
    }

    /// 是否含 `address`。
    pub(crate) fn contains(self, address: usize) -> bool {
        self.start.as_usize() <= address && address <= self.end()
    }

    /// 是否与 [lo, hi] 相交。
    pub(crate) fn overlaps(self, lo: usize, hi: usize) -> bool {
        self.start.as_usize() <= hi && self.end() >= lo
    }

    /// 与 [lo, hi] 的含尾交集；无交集返回 None。
    ///
    /// 派生等价用例直接调这一个原语做交集算术，不另写一份 max/min 镜像。
    pub(crate) fn intersection(self, lo: usize, hi: usize) -> Option<(usize, usize)> {
        let lo = self.start.as_usize().max(lo);
        let hi = self.end().min(hi);
        (lo <= hi).then_some((lo, hi))
    }

    /// ranges 是否连续覆盖 [lo, hi]。
    pub(crate) fn cover(ranges: &[Self], lo: usize, hi: usize) -> bool {
        let mut at = lo;
        for range in ranges {
            if range.end() < at {
                continue;
            }
            if range.start.as_usize() > at {
                return false;
            }
            if range.end() >= hi {
                return true;
            }
            at = range.end() + 1;
        }
        false
    }

    /// ranges 中覆盖 `address` 的区间当前权限。
    pub(crate) fn current_at(ranges: &[Self], address: usize) -> Option<Access> {
        ranges
            .iter()
            .find(|range| range.contains(address))
            .map(|range| range.current)
    }

    /// 去掉 [lo, hi]（`access = None`），或把交叠部分的当前权限换成 `access`。
    pub(crate) fn split(
        ranges: &[Self],
        lo: usize,
        hi: usize,
        access: Option<Access>,
    ) -> Result<Vec<Self>, NestError> {
        let mut out = Vec::new();
        out.try_reserve(ranges.len().saturating_add(2))
            .map_err(|_| NestError::OutOfMemory)?;
        for range in ranges {
            let rlo = range.start.as_usize();
            let rhi = range.end();
            if !range.overlaps(lo, hi) {
                out.push(*range);
                continue;
            }
            if rlo < lo {
                out.push(Self {
                    start: range.start,
                    size: NonZeroUsize::new(lo - rlo).unwrap(),
                    ..*range
                });
            }
            if let Some(access) = access {
                let part_lo = rlo.max(lo);
                let part_hi = rhi.min(hi);
                out.push(Self {
                    start: crate::machine::mmu::riscv::canonical(part_lo),
                    size: NonZeroUsize::new(part_hi - part_lo + 1).unwrap(),
                    current: access,
                    ..*range
                });
            }
            if rhi > hi {
                let part_lo = hi + 1;
                out.push(Self {
                    start: crate::machine::mmu::riscv::canonical(part_lo),
                    size: NonZeroUsize::new(rhi - part_lo + 1).unwrap(),
                    ..*range
                });
            }
        }
        Ok(out)
    }

    /// 收窄 [lo, hi]：ceiling 换成 `ceiling`，current 收窄为它与 ceiling 的交集。
    /// 交叠区间的原 ceiling 不容纳新 ceiling、或结果不是合法叶子权限 ⇒ WidenDenied。
    pub(crate) fn narrow(
        ranges: &[Self],
        lo: usize,
        hi: usize,
        ceiling: Access,
    ) -> Result<Vec<Self>, NestError> {
        let mut next = Vec::new();
        next.try_reserve(ranges.len().saturating_add(2))
            .map_err(|_| NestError::OutOfMemory)?;
        for range in ranges {
            let rlo = range.start.as_usize();
            let rhi = range.end();
            if !range.overlaps(lo, hi) {
                next.push(*range);
                continue;
            }
            if !range.ceiling.contains(ceiling) {
                return Err(NestError::WidenDenied);
            }
            if rlo < lo {
                next.push(Self {
                    size: NonZeroUsize::new(lo - rlo).unwrap(),
                    ..*range
                });
            }
            let mid_lo = rlo.max(lo);
            let mid_hi = rhi.min(hi);
            let current = range.current & ceiling;
            if !current.is_leaf_set() {
                return Err(NestError::WidenDenied);
            }
            next.push(Self {
                start: crate::machine::mmu::riscv::canonical(mid_lo),
                size: NonZeroUsize::new(mid_hi - mid_lo + 1).unwrap(),
                ceiling,
                current,
                ..*range
            });
            if rhi > hi {
                let right = hi + 1;
                next.push(Self {
                    start: crate::machine::mmu::riscv::canonical(right),
                    size: NonZeroUsize::new(rhi - right + 1).unwrap(),
                    ..*range
                });
            }
        }
        Ok(next)
    }
}
