//! 选址窗口与选址规则。在同一个 Nest 锁下选择空段并登记占用。

use alloc::vec::Vec;

use super::AddressError;
use super::occupancy::Occupancy;
use super::placement::{Placement, PlacementAt};

/// 地址区域：`Normal` / `Kernel` / `Resource` 各有一个动态选址窗口；
/// `Static` 只是地址记账政策：它没有动态窗口，跨度只按 `Exact` 登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionKind {
    Normal,
    Kernel,
    Resource,
    Static,
}

/// 一个动态选址窗口；`last` 是最后一个可用地址（含）。
#[derive(Clone, Copy)]
struct Window {
    kind: RegionKind,
    base: usize,
    last: usize,
}

pub(crate) struct Layout {
    windows: Vec<Window>,
    pub(crate) occupancy: Occupancy,
}

impl Layout {
    pub(crate) const fn new() -> Self {
        Self {
            windows: Vec::new(),
            occupancy: Occupancy::new(),
        }
    }

    /// 登记动态窗口 `[base, last]`（含端点）。
    ///
    /// `Static` 只是地址记账政策，没有动态窗口：给这类区域建窗就是 NoRegion。
    pub(crate) fn register_window(
        &mut self,
        kind: RegionKind,
        base: usize,
        last: usize,
    ) -> Result<(), AddressError> {
        if base > last
            || kind == RegionKind::Static
            || self.windows.iter().any(|window| window.kind == kind)
        {
            return Err(AddressError::NoRegion);
        }
        self.windows
            .try_reserve(1)
            .map_err(|_| AddressError::OutOfMemory)?;
        self.windows.push(Window { kind, base, last });
        Ok(())
    }

    /// 抬高已有窗口的选址下界；已登记的占用留在表里。
    pub(crate) fn advance_window(
        &mut self,
        kind: RegionKind,
        base: usize,
    ) -> Result<(), AddressError> {
        let window = self
            .windows
            .iter_mut()
            .find(|window| window.kind == kind)
            .ok_or(AddressError::NoRegion)?;
        if base < window.base || base > window.last {
            return Err(AddressError::NoRegion);
        }
        window.base = base;
        Ok(())
    }

    /// 按 `placement` 选段并登记占用：不分配数据、不发布 PTE。
    /// 返回选中的起点与占用身份（generation）。
    pub(crate) fn reserve(&mut self, placement: Placement) -> Result<(usize, usize), AddressError> {
        let size = placement.size();
        let region = placement.region();
        let start = match placement.at() {
            PlacementAt::Exact(start) => start.as_usize(),
            PlacementAt::Lowest => {
                let window = self.window(region)?;
                let align = placement.align();
                self.occupancy
                    .free_spans(window.base, window.last)?
                    .into_iter()
                    .find_map(|(lo, hi)| {
                        let at = lo.checked_next_multiple_of(align)?;
                        hi.checked_sub(at)
                            .is_some_and(|gap| gap >= size - 1)
                            .then_some(at)
                    })
                    .ok_or(AddressError::OutOfMemory)?
            }
            PlacementAt::Highest => {
                let window = self.window(region)?;
                self.occupancy
                    .free_spans(window.base, window.last)?
                    .into_iter()
                    .rev()
                    .find_map(|(lo, hi)| {
                        let at = hi.checked_sub(size - 1)?;
                        (at >= lo).then_some(at)
                    })
                    .ok_or(AddressError::OutOfMemory)?
            }
        };
        let last = start.checked_add(size - 1).ok_or(AddressError::NoRegion)?;
        let id = self.occupancy.insert(start, last)?;
        Ok((start, id))
    }

    fn window(&self, kind: RegionKind) -> Result<Window, AddressError> {
        self.windows
            .iter()
            .copied()
            .find(|window| window.kind == kind)
            .ok_or(AddressError::NoRegion)
    }

    /// 窗口 `[base, last]` 是否含住 `[start, last]`。
    pub(crate) fn window_contains(&self, kind: RegionKind, start: usize, last: usize) -> bool {
        self.window(kind)
            .is_ok_and(|window| start >= window.base && last <= window.last)
    }
}
