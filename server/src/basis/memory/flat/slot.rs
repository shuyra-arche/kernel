use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::basis::memory::address::PageIndex;

use super::error::FlatError;
use crate::basis::memory::cell::Cell;

/// 消费唯一 Cell，建立新槽。不会接受映射保留的 Arc<Cell>，也不会复制物理实例。
/// 准备失败时，局部槽和剩余输入一起释放；没有已发布的半份导入。
pub(super) fn admit(
    page_count: usize,
    cells: Vec<(PageIndex, Cell)>,
) -> Result<Vec<(PageIndex, Slot)>, FlatError> {
    let mut previous = None;
    for (index, _) in &cells {
        let page = index.get();
        if page >= page_count || previous.is_some_and(|previous| previous >= page) {
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
/// `Moving` 保留本次移交的 Cell 与事务身份，同时封闭正常物化/发布，
/// 于是即使中途 abort，这一页也不会失去它的承载（fail-closed）。
/// `Reserved` 是“一次移交给这一页留了空位”，尚未交给承载。
/// 两者都不算空闲态：物化、PA 出口与发布都与它们互斥。
pub(super) enum Slot {
    Resident(Arc<Cell>),
    Missing,
    Moving {
        cell: Arc<Cell>,
        transaction: usize,
    },
    /// 一次移交给这一页留的空位；记着它原本是 Fresh 还是 `Missing`，
    /// 于是事务中途回滚能把这一页原样还回去。
    Reserved {
        transaction: usize,
        was_missing: bool,
    },
}

/// 页槽在按页号严格递增的表里的位置，以及这一页是不是已经占了槽。
pub(super) fn spot(slots: &[(PageIndex, Slot)], page: PageIndex) -> (usize, bool) {
    let at = slots.partition_point(|(slot, _)| slot.get() < page.get());
    (
        at,
        slots
            .get(at)
            .is_some_and(|(slot, _)| slot.get() == page.get()),
    )
}
