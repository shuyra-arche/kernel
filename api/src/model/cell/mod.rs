//! Memory identities, ranges and placement descriptors.

mod allocation;
mod mapping_control;
mod mapping_id;
mod page_owner;
mod placement;
mod placement_at;
mod placement_error;
mod reference_range;
mod slot_id;

pub use allocation::Allocation;
pub use mapping_control::MappingControl;
pub use mapping_id::MappingId;
pub use page_owner::PageOwner;
pub use placement::Placement;
pub use placement_at::PlacementAt;
pub use placement_error::PlacementError;
pub use reference_range::ReferenceRange;
pub use slot_id::SlotId;

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

mod mapping_target;
pub use mapping_target::MappingTarget;

mod object;
pub use object::{
    CellCreate, CellHandoff, CellInfo, CellObject, CellPosition, CellSource, CellState, CreatedCell,
};
