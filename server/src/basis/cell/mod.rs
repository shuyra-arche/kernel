pub(crate) mod page;

use self::page::{Owner, Page, PageError};
use crate::basis::flat::FlatError;
use kernel_api::model::cell::CellLevel;
pub(crate) mod change;
pub(crate) fn allocate(owner: Owner) -> Result<Page, FlatError> {
    allocate_level(owner, CellLevel::BASE)
}
pub(crate) fn allocate_level(owner: Owner, level: CellLevel) -> Result<Page, FlatError> {
    crate::tag!(Plain, unsafe {
        Page::allocate(
            owner,
            level,
            crate::metal::allocator::frame::instance(),
            crate::metal::allocator::frame::repartition,
        )
    })
    .map_err(|error| match error {
        PageError::InvalidOwner => FlatError::OutOfRange,
        PageError::InvalidRange => FlatError::NotAligned,
        PageError::IdentityExhausted | PageError::OutOfMemory => FlatError::OutOfMemory,
        PageError::Busy => FlatError::Busy,
    })
}
