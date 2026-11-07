use crate::basis::nest::memo::Memo;
use alloc::vec::Vec;

use crate::basis::flat::FlatRange;

use super::active::ActiveRange;
use super::runs::CellRuns;

/// 一段活动关系的库存：活动区间、强持的平来源、已发布页的强持记录。
pub(crate) struct MappingState {
    pub(crate) active: Vec<ActiveRange>,
    pub(crate) source: FlatRange,
    pub(crate) published: Vec<(usize, Memo)>,
}

impl MappingState {
    pub(crate) fn new(active: Vec<ActiveRange>, source: FlatRange) -> Self {
        Self {
            active,
            source,
            published: Vec::new(),
        }
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
    pub(crate) fn publish(&mut self, page: usize, cell: Memo) {
        let at = self.published.partition_point(|(held, _)| *held < page);
        self.published.insert(at, (page, cell));
    }

    /// 帧分段：以 `base` 为虚拟起点、按活动区间 current 与物理连续性切段。
    pub(crate) fn runs<'a>(
        &'a self,
        base: kernel_api::address::VirtAddr,
        cells: &'a [(usize, Memo)],
    ) -> CellRuns<'a> {
        CellRuns::new(base, cells, &self.active)
    }
}
