use super::Access;
use crate::basis::nest::NestError;
use crate::basis::nest::publication::Publication;
use alloc::vec::Vec;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::VirtAddr;

use crate::basis::flat::FlatRange;

use super::active::ActiveRange;
use super::memo::Memo;
use super::runs::CellRuns;

// 映射改权的纯决策：只读取逻辑状态，返回新活动区间和页表更新意图。
// Nest 在同一持锁区间内准备硬件资源、提交并结算失效；这里不获取锁或操作页表。

#[derive(Clone, Copy)]
pub(crate) enum Protection {
    SetAccess(Access),
    Narrow(Access),
}

pub(crate) struct PermissionUpdate {
    pub address: VirtAddr,
    pub access: Access,
}

pub(crate) struct ProtectionTransition {
    pub active: Vec<ActiveRange>,
    pub updates: Vec<PermissionUpdate>,
}

/// 一段活动关系的库存：活动区间、强持的平来源、已发布页的强持记录。
pub(crate) struct MappingState {
    pub(crate) active: Vec<ActiveRange>,
    pub(crate) source: FlatRange,
    pub(crate) published: Vec<(usize, Publication)>,
    pub(crate) explicit: bool,
    pub(crate) memos: Vec<(usize, Memo)>,
}

impl MappingState {
    pub(crate) fn new(active: Vec<ActiveRange>, source: FlatRange) -> Self {
        Self {
            active,
            source,
            published: Vec::new(),
            explicit: false,
            memos: Vec::new(),
        }
    }

    pub(crate) fn memo(&self, page: usize) -> Option<&Memo> {
        self.memos
            .iter()
            .find(|(index, _)| *index == page)
            .map(|(_, memo)| memo)
    }
    pub(crate) fn memo_mut(
        &mut self,
        page: usize,
    ) -> Result<&mut Memo, crate::basis::nest::NestError> {
        if let Some(index) = self.memos.iter().position(|(index, _)| *index == page) {
            return Ok(&mut self.memos[index].1);
        }
        self.memos
            .try_reserve(1)
            .map_err(|_| crate::basis::nest::NestError::OutOfMemory)?;
        self.memos.push((page, Memo::vacant()));
        Ok(&mut self.memos.last_mut().unwrap().1)
    }
    /// 活动区间是否连续覆盖 [lo, hi]。
    pub(crate) fn covers(&self, lo: usize, hi: usize) -> bool {
        ActiveRange::cover(&self.active, lo, hi)
    }

    /// 覆盖 `address` 的活动区间。
    pub(crate) fn range_at(&self, address: usize) -> Option<&ActiveRange> {
        self.active.iter().find(|range| range.contains(address))
    }

    /// 该页是否已有发布记录。
    pub(crate) fn publishes(&self, page: usize) -> bool {
        self.published.iter().any(|(held, _)| *held == page)
    }

    /// 按页号顺序插入发布记录。
    pub(crate) fn publish(&mut self, page: usize, cell: Publication) {
        let at = self.published.partition_point(|(held, _)| *held < page);
        self.published.insert(at, (page, cell));
    }

    /// 帧分段：以 `base` 为虚拟起点、按活动区间 current 与物理连续性切段。
    pub(crate) fn runs<'a>(
        &'a self,
        base: kernel_api::address::VirtAddr,
        cells: &'a [(usize, Publication)],
    ) -> CellRuns<'a> {
        CellRuns::new(base, cells, &self.active)
    }

    /// Historical observers may retain Mapping after deletion. They must not retain its Pages.
    pub(crate) fn settle_deleted_memos(&mut self, base: usize) {
        let active = &self.active;
        self.memos.retain(|(index, _)| {
            active
                .iter()
                .any(|r| r.contains(base + index * kernel_api::address::PAGE_SIZE))
        });
    }
    /// `base` 与 `window` 来自同一 MappingRange；窗口非空、页对齐且位于初始范围。
    /// 全程只读：拒绝或分配失败都不修改逻辑权限，也不发布任何硬件权限。
    pub(crate) fn protection_transition(
        &self,
        base: VirtAddr,
        (lo, hi): (usize, usize),
        request: Protection,
    ) -> Result<ProtectionTransition, NestError> {
        if let Protection::SetAccess(access) = request
            && !access.is_leaf_set()
        {
            return Err(NestError::NotAligned);
        }
        if !self.covers(lo, hi) {
            return Err(NestError::NoRegion);
        }
        let active = match request {
            Protection::SetAccess(access) => {
                if self
                    .active
                    .iter()
                    .filter(|range| range.overlaps(lo, hi))
                    .any(|range| !range.ceiling.contains(access))
                {
                    return Err(NestError::WidenDenied);
                }
                ActiveRange::split(&self.active, lo, hi, Some(access))?
            }
            Protection::Narrow(ceiling) => ActiveRange::narrow(&self.active, lo, hi, ceiling)?,
        };
        let first = (lo - base.as_usize()) / PAGE_SIZE;
        let pages = (hi - lo + 1) / PAGE_SIZE;
        let borrowed = self.source.is_borrowed();
        let mut updates = Vec::new();
        updates
            .try_reserve(pages)
            .map_err(|_| NestError::OutOfMemory)?;
        for page in first..first + pages {
            let address = base + page * PAGE_SIZE;
            let access =
                ActiveRange::current_at(&active, address.as_usize()).ok_or(NestError::NoRegion)?;
            if borrowed || self.publishes(page) {
                updates.push(PermissionUpdate { address, access });
            }
        }
        Ok(ProtectionTransition { active, updates })
    }
}
