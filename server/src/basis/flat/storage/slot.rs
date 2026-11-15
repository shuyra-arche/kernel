use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::address::PageIndex;

use super::super::FlatError;
use crate::basis::cell::page::Page;

/// 消费唯一 Page，建立新槽。不会接受映射保留的 Arc<Page>，也不会复制物理实例。
/// 准备失败时，局部槽和剩余输入一起释放；没有已发布的半份导入。
pub(in super::super) fn admit(
    page_count: usize,
    cells: Vec<(PageIndex, Page)>,
) -> Result<Vec<(PageIndex, Slot)>, FlatError> {
    let mut previous = None;
    for (index, cell) in &cells {
        let page = index.get();
        if cell.pages() != 1
            || page >= page_count
            || previous.is_some_and(|previous| previous >= page)
        {
            return Err(FlatError::OutOfRange);
        }
        previous = Some(page);
    }
    let mut slots = Vec::new();
    slots
        .try_reserve_exact(cells.len())
        .map_err(|_| FlatError::OutOfMemory)?;
    for (index, cell) in cells {
        let cell = Arc::try_new(cell).map_err(|_| FlatError::OutOfMemory)?;
        slots.push((index, Slot::Resident(cell)));
    }
    Ok(slots)
}

/// 一个逻辑页槽的状态；页槽表里缺席即 Fresh（从未物化）。
///
/// `Resident` 是可供正常物化/发布的驻留态；`Missing` 是承载已移走的墓碑，
/// 不得再自动补齐（只有移交能把新承载交进它）。
pub(in super::super) enum Slot {
    Resident(Arc<Page>),
    Missing,
    /// A fresh position exclusively retained by an off-lock allocation. Only
    /// its reservation can publish or restore Fresh; transfers see Busy.
    Allocating {
        reservation: usize,
    },
}

/// 页槽在按页号严格递增的表里的位置，以及这一页是不是已经占了槽。
pub(in super::super) fn spot(slots: &[(PageIndex, Slot)], page: PageIndex) -> (usize, bool) {
    let at = slots.partition_point(|(slot, _)| slot.get() < page.get());
    (
        at,
        slots
            .get(at)
            .is_some_and(|(slot, _)| slot.get() == page.get()),
    )
}
