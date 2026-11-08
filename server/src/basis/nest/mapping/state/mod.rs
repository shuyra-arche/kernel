mod memo;

use super::publication::{CellRuns, Publication};
use super::{Access, ActiveRange};
use crate::basis::cell::page::{Change, Page};
use crate::basis::nest::NestError;
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::VirtAddr;

use crate::basis::flat::FlatRange;

use self::memo::Memo;

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
    // Reserved before publication so failed withdrawal can queue the Mapping
    // without allocating or making its address reusable.
    pub(in crate::basis::nest) cleanup: Option<Box<super::super::CleanupNode>>,
    pub(crate) active: Vec<ActiveRange>,
    pub(crate) source: FlatRange,
    pub(crate) published: Vec<(usize, Publication)>,
    pub(crate) explicit: bool,
    memos: Vec<(usize, Memo)>,
}

impl MappingState {
    pub(crate) fn new(active: Vec<ActiveRange>, source: FlatRange) -> Self {
        Self {
            cleanup: None,
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
    pub(super) fn memo_mut(
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
    pub(crate) fn memo_page(&self, page: usize) -> Option<Arc<Page>> {
        if let Some(memo) = self.memo(page) {
            return memo.page.clone();
        }
        self.published
            .iter()
            .find(|(index, _)| *index == page)
            .map(|(_, publication)| publication.cell().clone())
    }

    pub(crate) fn associate(
        &mut self,
        base: VirtAddr,
        index: usize,
        expected: u64,
        page: Arc<Page>,
    ) -> Result<(), NestError> {
        self.memo_address(base, index)?;
        if self.publishes(index) {
            return Err(NestError::AlreadyMapped);
        }
        self.memo_mut(index)?.associate(expected, page)
    }

    pub(crate) fn inspect_memo(
        &self,
        base: VirtAddr,
        index: usize,
    ) -> Result<kernel_api::model::cell::CellInfo, NestError> {
        use kernel_api::model::cell::{CellInfo, CellState};
        let address = self.memo_address(base, index)?;
        let memo = self.memo(index);
        let publication = self
            .published
            .iter()
            .find(|(p, _)| *p == index)
            .map(|(_, p)| p);
        let page = memo
            .and_then(|m| m.page.as_ref())
            .or_else(|| publication.map(|p| p.cell()));
        let state = memo
            .and_then(|m| m.flight.map(|flight| flight.observation()))
            .unwrap_or(if publication.is_some() {
                CellState::Published
            } else if page.is_some() {
                CellState::Ready
            } else {
                CellState::Vacant
            });
        Ok(CellInfo {
            generation: memo.map_or(1, |m| m.generation.get()),
            state,
            page: page.map_or(0, |p| p.id().get() as u64),
            frozen: page.is_some_and(|p| p.frozen()),
            external: page
                .is_some_and(|p| p.exposure() == crate::basis::cell::page::Exposure::External),
            address: VirtAddr::new(address),
            fault_cause: 0,
            fault_address: 0,
        })
    }

    pub(crate) fn transition_memo(
        &mut self,
        index: usize,
        expected: u64,
        freeze: Arc<Change>,
        publishing: bool,
    ) -> Result<Arc<Page>, NestError> {
        let published = self
            .published
            .iter()
            .find(|(p, _)| *p == index)
            .map(|(_, p)| p.cell().clone());
        self.memo_mut(index)?
            .transition(expected, published, freeze, publishing)
    }

    pub(crate) fn finish_memo(&mut self, index: usize) {
        let published = self.publishes(index);
        if let Ok(memo) = self.memo_mut(index) {
            memo.finish(published);
        }
    }

    fn memo_address(&self, base: VirtAddr, index: usize) -> Result<usize, NestError> {
        let address = base
            .get()
            .checked_add(index.checked_mul(PAGE_SIZE).ok_or(NestError::NoRegion)?)
            .ok_or(NestError::NoRegion)?;
        self.range_at(address).ok_or(NestError::NoRegion)?;
        Ok(address)
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
