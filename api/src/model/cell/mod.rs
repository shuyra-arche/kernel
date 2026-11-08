//! Memory identities, ranges and placement descriptors.

mod allocation;
mod mapping;
mod object;
mod owner;
mod placement;
mod range;
mod slot;

pub use allocation::Allocation;
pub use mapping::{MappingControl, MappingId, MappingTarget};
pub use object::{
    CellCreate, CellHandoff, CellInfo, CellObject, CellPosition, CellSource, CellState, CreatedCell,
};
pub use owner::PageOwner;
pub use placement::{Placement, PlacementAt, PlacementError};
pub use range::ReferenceRange;
pub use slot::SlotId;

/// 环境内存调用与装载格式使用的页粒度。
pub use crate::address::PAGE_SIZE;

/// 范围坐标的唯一 checked 口径：`count` 非零，且偏移、页数与字节跨度都不溢出。
///
/// Descriptors share this checked range geometry,
/// 线外不接受靠溢出缩短范围的输入。
pub(crate) fn pages(offset: usize, count: usize) -> Option<()> {
    if count == 0 {
        return None;
    }
    count.checked_mul(PAGE_SIZE)?;
    let end = offset.checked_add(count)?;
    end.checked_mul(PAGE_SIZE)?;
    Some(())
}
