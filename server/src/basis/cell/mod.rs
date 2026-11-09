pub(crate) mod page;
pub(crate) mod transfer;

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

/// A resource collection; its organization determines the meaning of a position.
pub(crate) enum Cell {
    Flat(crate::basis::flat::FlatRange),
    Nest(crate::basis::nest::NestRange),
}

impl Cell {
    pub(crate) fn page(
        &self,
        offset: usize,
    ) -> Result<alloc::sync::Arc<Page>, crate::basis::nest::NestError> {
        use crate::basis::nest::NestError;
        match self {
            Self::Flat(range) => range
                .cell(kernel_api::address::PageOffset::new(offset))?
                .materialized_cells()?
                .into_iter()
                .next()
                .map(|(_, page)| page)
                .ok_or(NestError::Missing),
            Self::Nest(range) => {
                let mapping = range.mapping();
                let (start, end) = range.window();
                let address = start
                    .checked_add(
                        offset
                            .checked_mul(kernel_api::address::PAGE_SIZE)
                            .ok_or(NestError::NoRegion)?,
                    )
                    .filter(|a| *a <= end)
                    .ok_or(NestError::NoRegion)?;
                let page = mapping.page_index(kernel_api::address::VirtAddr::new(address));
                let nest = mapping.owner().ok_or(NestError::NoRegion)?;
                nest.memo_page(mapping, page)?.ok_or(NestError::Missing)
            }
        }
    }
}
