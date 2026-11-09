//! Memory identities, ranges and placement descriptors.

mod allocation;
mod creation;
mod info;
mod level;
mod mapping;
mod object;
mod placement;
mod slot;

pub use allocation::Allocation;
pub use creation::{CellCreate, CellSource, CreatedCell, MemoDestination};
pub use info::{CellInfo, CellState};
pub use level::CellLevel;
pub use mapping::{MappingControl, MappingId, MappingTarget};
pub use object::{CellHandoff, CellObject, CellPosition};
pub use placement::{Placement, PlacementAt, PlacementError};
pub use slot::{ReferenceRange, SlotId};

/// Existing alias for the 4 KiB ABI coordinate. This is not physical Cell geometry.
pub use crate::address::PAGE_SIZE;

/// A protection domain owns managed memory. Task membership and management
/// authority do not create additional owners of the same physical Page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageOwner {
    Kernel,
    Team(crate::model::identity::TeamId),
}

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
