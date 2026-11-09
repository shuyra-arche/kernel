//! Exclusive physical receipt repartition. All fallible work precedes commit.
use super::{Page, PageError, PageId, Region, reclaim};
use alloc::{sync::Arc, vec::Vec};
use core::{alloc::Layout, mem::ManuallyDrop, ptr::NonNull};
use kernel_api::model::cell::CellLevel;
impl Page {
    pub(crate) fn reshape(
        pages: &mut Vec<Arc<Self>>,
        level: CellLevel,
    ) -> Result<Vec<Arc<Self>>, PageError> {
        let first = pages.first().ok_or(PageError::InvalidRange)?;
        let old = first.level;
        let size = level.bytes().ok_or(PageError::InvalidRange)?;
        let total = pages
            .len()
            .checked_mul(first.size())
            .ok_or(PageError::InvalidRange)?;
        let base = first.physical_address();
        if base.checked_add(total).is_none()
            || old == level
            || !base.is_multiple_of(size)
            || !total.is_multiple_of(size)
        {
            return Err(PageError::InvalidRange);
        }
        let receipt = first.page.as_ref().unwrap();
        let repartition = receipt.repartition.ok_or(PageError::InvalidRange)?;
        for (index, page) in pages.iter().enumerate() {
            if Arc::strong_count(page) != 1 || Arc::weak_count(page) != 0 || !page.allows_return() {
                return Err(PageError::Busy);
            }
            if page.level != old
                || page.owner() != first.owner()
                || page.physical_address() != base + index * first.size()
                || !receipt.compatible(page.page.as_ref().unwrap())
            {
                return Err(PageError::InvalidRange);
            }
        }
        let count = total / size;
        let layout = Layout::from_size_align(size, size).map_err(|_| PageError::InvalidRange)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| PageError::OutOfMemory)?;
        for _ in 0..count {
            result.push(
                Arc::try_new(Self {
                    id: PageId::allocate().ok_or(PageError::IdentityExhausted)?,
                    owner: first.owner.clone(),
                    level,
                    page: None,
                    reclaim: reclaim::Reclaim::new(),
                })
                .map_err(|_| PageError::OutOfMemory)?,
            );
        }
        let allocator = receipt.allocator;
        // SAFETY: exclusive, aligned receipts cover exactly the same contiguous union.
        unsafe { repartition(base, old, pages.len(), level, count) };
        for page in pages.iter_mut() {
            let _receipt = ManuallyDrop::new(Arc::get_mut(page).unwrap().page.take().unwrap());
        }
        for (index, page) in result.iter_mut().enumerate() {
            Arc::get_mut(page).unwrap().page = Some(Region {
                pointer: NonNull::new((base + index * size) as *mut u8).unwrap(),
                layout,
                allocator,
                repartition: Some(repartition),
            });
        }
        pages.clear();
        Ok(result)
    }
}
