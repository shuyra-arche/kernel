//! Memory identities, ranges and placement descriptors.

mod acquire_source;
mod allocation;
mod cell_owner;
mod flat_reference;
mod mapping_control;
mod mapping_id;
mod mapping_selection;
mod placement;
mod placement_at;
mod placement_error;
mod reference_range;

pub use acquire_source::AcquireSource;
pub use allocation::Allocation;
pub use cell_owner::CellOwner;
pub use flat_reference::FlatReference;
pub use mapping_control::MappingControl;
pub use mapping_id::MappingId;
pub use mapping_selection::MappingSelection;
pub use placement::Placement;
pub use placement_at::PlacementAt;
pub use placement_error::PlacementError;
pub use reference_range::ReferenceRange;

/// 环境内存调用与装载格式使用的页粒度。
pub const PAGE_SIZE: usize = 4096;

/// 范围坐标的唯一 checked 口径：`count` 非零，且偏移、页数与字节跨度都不溢出。
///
/// 页窗口的两处用家（[`ReferenceRange`] 与 [`MappingSelection`]）共用它，
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
