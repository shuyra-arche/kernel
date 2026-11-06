//! Memory identities, ranges and placement descriptors.

mod acquire_source;
mod allocation;
mod flat_reference;
mod placement;
mod placement_at;
mod placement_error;
mod reference_range;
mod view_control;
mod view_id;
mod view_selection;

pub use acquire_source::AcquireSource;
pub use allocation::Allocation;
pub use flat_reference::FlatReference;
pub use placement::Placement;
pub use placement_at::PlacementAt;
pub use placement_error::PlacementError;
pub use reference_range::ReferenceRange;
pub use view_control::ViewControl;
pub use view_id::ViewId;
pub use view_selection::ViewSelection;

/// 环境内存调用与装载格式使用的页粒度。
pub const PAGE_SIZE: usize = 4096;

/// 范围坐标的唯一 checked 口径：`count` 非零，且偏移、页数与字节跨度都不溢出。
///
/// 页窗口的两处用家（[`ReferenceRange`] 与 [`ViewSelection`]）共用它，
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
