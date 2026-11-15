pub(crate) mod change;
pub(crate) mod page;

use self::page::{Owner, Page, PageError};
use kernel_api::model::cell::CellLevel;

pub(crate) fn allocate(owner: Owner) -> Result<Page, PageError> {
    allocate_level(owner, CellLevel::BASE)
}

pub(crate) fn allocate_level(owner: Owner, level: CellLevel) -> Result<Page, PageError> {
    crate::tag!(Plain, unsafe {
        Page::allocate(
            owner,
            level,
            crate::metal::allocator::frame::instance(),
            crate::metal::allocator::frame::repartition,
        )
    })
}
