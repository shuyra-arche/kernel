pub(crate) mod page;
pub(crate) mod transfer;

use self::page::{InternalPage, Owner, Page, PageError};
use crate::basis::flat::FlatError;
use alloc::alloc::Allocator;
use alloc::boxed::Box;
pub(crate) mod change;
/// 分配器一侧分配并清零 RAM，再将唯一归还凭证交给 Page。
pub(crate) fn allocate(owner: Owner) -> Result<Page, FlatError> {
    let allocator: &'static (dyn Allocator + Sync) = crate::machine::allocator::frame::instance();
    let page: InternalPage = crate::tag!(Plain, unsafe {
        Box::try_new_zeroed_in(allocator)
            .map_err(|_| FlatError::OutOfMemory)?
            .assume_init()
    });
    Page::from_internal(owner, page).map_err(|error| match error {
        PageError::InvalidOwner => FlatError::OutOfRange,
        PageError::InvalidRange => FlatError::NotAligned,
        PageError::IdentityExhausted => FlatError::OutOfMemory,
    })
}

/// A resource collection; its organization determines the meaning of a position.
pub(crate) enum Cell {
    Flat(crate::basis::flat::FlatRange),
    Nest(crate::basis::nest::MappingRange),
}

impl Cell {
    pub(crate) fn page(
        &self,
        offset: usize,
    ) -> Result<alloc::sync::Arc<Page>, crate::basis::nest::NestError> {
        use crate::basis::nest::NestError;
        match self {
            Self::Flat(range) => range
                .range(
                    kernel_api::address::PageOffset::new(offset),
                    kernel_api::address::PageCount::new(1).unwrap(),
                )?
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
