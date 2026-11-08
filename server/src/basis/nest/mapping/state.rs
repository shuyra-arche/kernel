use crate::basis::nest::publication::Publication;
use alloc::vec::Vec;

use crate::basis::flat::FlatRange;

use super::active::ActiveRange;
use super::runs::CellRuns;

/// 一段活动关系的库存：活动区间、强持的平来源、已发布页的强持记录。
pub(crate) struct MappingState {
    pub(crate) active: Vec<ActiveRange>,
    pub(crate) source: FlatRange,
    pub(crate) published: Vec<(usize, Publication)>,
    pub(crate) explicit: bool,
    pub(crate) memos: Vec<(usize, super::super::memo::Memo)>,
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

    pub(crate) fn memo(&self, page: usize) -> Option<&super::super::memo::Memo> {
        self.memos
            .iter()
            .find(|(index, _)| *index == page)
            .map(|(_, memo)| memo)
    }
    pub(crate) fn memo_mut(
        &mut self,
        page: usize,
    ) -> Result<&mut super::super::memo::Memo, crate::basis::nest::NestError> {
        if let Some(index) = self.memos.iter().position(|(index, _)| *index == page) {
            return Ok(&mut self.memos[index].1);
        }
        self.memos
            .try_reserve(1)
            .map_err(|_| crate::basis::nest::NestError::OutOfMemory)?;
        self.memos.push((page, super::super::memo::Memo::vacant()));
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
}

impl MappingState {
    /// Historical observers may retain Mapping after deletion. They must not retain its Pages.
    pub(crate) fn settle_deleted_memos(&mut self, base: usize) {
        let active = &self.active;
        self.memos.retain(|(index, _)| {
            active
                .iter()
                .any(|r| r.contains(base + index * kernel_api::address::PAGE_SIZE))
        });
    }
}
