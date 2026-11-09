use super::Publication;
use crate::basis::nest::Access;
use crate::basis::nest::ActiveRange;
use crate::basis::nest::NestError;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;

/// 帧分段迭代器：把 `cells` 切成一串连续段，不再分配。
///
/// 续段条件同时成立：页号相邻、物理地址相邻、所在区间 current 相同。
/// 段起点虚拟地址为 `base + 帧页号 * PAGE_SIZE`；任一帧不在任何区间内 ⇒ NoRegion。
/// 一段连续帧：虚拟连续、物理连续，且落在同一个当前权限相同的区间内。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellRun {
    pub(crate) start: VirtAddr,
    pub(crate) physical: PhysAddr,
    pub(crate) size: usize,
    pub(crate) current: Access,
}

pub(crate) struct CellRuns<'a> {
    base: VirtAddr,
    cells: &'a [(usize, Publication)],
    active: &'a [ActiveRange],
    at: usize,
}

impl<'a> CellRuns<'a> {
    pub(crate) fn new(
        base: VirtAddr,
        cells: &'a [(usize, Publication)],
        active: &'a [ActiveRange],
    ) -> Self {
        Self {
            base,
            cells,
            active,
            at: 0,
        }
    }

    fn address(&self, page: usize) -> VirtAddr {
        self.base + page * PAGE_SIZE
    }

    /// 第 `at` 帧能否续在 current 的段后面。
    fn joins(&self, at: usize, current: Access) -> bool {
        let previous = &self.cells[at - 1];
        let next = &self.cells[at];
        next.0 == previous.0 + previous.1.cell().pages()
            && next.1.physical_address() == previous.1.physical_address() + previous.1.cell().size()
            && ActiveRange::current_at(self.active, self.address(next.0).as_usize())
                .map(|access| access.bits())
                == Some(current.bits())
    }
}

impl Iterator for CellRuns<'_> {
    type Item = Result<CellRun, NestError>;

    fn next(&mut self) -> Option<Self::Item> {
        let first = self.at;
        if first >= self.cells.len() {
            return None;
        }
        self.at = first + 1;
        let current = match ActiveRange::current_at(
            self.active,
            self.address(self.cells[first].0).as_usize(),
        ) {
            Some(current) => current,
            None => return Some(Err(NestError::NoRegion)),
        };
        let mut last = first + 1;
        while last < self.cells.len() && self.joins(last, current) {
            last += 1;
        }
        self.at = last;
        Some(Ok(CellRun {
            start: self.address(self.cells[first].0),
            physical: PhysAddr::from_raw(self.cells[first].1.physical_address()),
            size: self.cells[first..last]
                .iter()
                .map(|(_, cell)| cell.cell().size())
                .sum(),
            current,
        }))
    }
}
