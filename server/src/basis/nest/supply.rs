//! Explicit Page publication into a pre-authorized Memo position.
use super::memo::Memo;
use super::{Access, Mapping, Nest, NestError};
use crate::basis::cell::page::{Change, Page};
use alloc::sync::Arc;
use kernel_api::address::{PAGE_SIZE, VirtAddr};

impl Nest {
    pub(crate) fn memo_page(
        &self,
        mapping: &Arc<Mapping>,
        page: usize,
    ) -> Result<Option<Arc<Page>>, NestError> {
        let state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        Ok(mapping
            .state
            .lock()
            .published
            .iter()
            .find(|(index, _)| *index == page)
            .map(|(_, memo)| memo.cell().clone()))
    }

    pub(crate) fn push_page(
        self: &Arc<Self>,
        mapping: &Arc<Mapping>,
        address: VirtAddr,
        page: &Arc<Page>,
        change: &Change,
    ) -> Result<(), NestError> {
        self.finish_invalidation()?;
        let mut state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let mut inner = mapping.state.lock();
        let index = mapping.page_index(address);
        if inner.publishes(index) {
            return Err(NestError::AlreadyMapped);
        }
        let range = inner
            .active
            .iter()
            .find(|range| range.contains(address.as_usize()))
            .ok_or(NestError::NoRegion)?;
        let flags = mapping.leaf_flags(range.current);
        let mut memo = Memo::prepare(page, range.ceiling.contains(Access::WRITE), Some(change))?;
        inner
            .published
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        state
            .pending_mappings
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        let physical = kernel_api::address::PhysAddr::from_raw(page.physical_address());
        state
            .root
            .prepare_range_geometry(address, physical, PAGE_SIZE, self.geometry)?;
        memo.publish();
        state
            .root
            .map_range_prepared_geometry(address, physical, PAGE_SIZE, flags, self.geometry);
        inner.publish(index, memo);
        state.pending_mappings.push(mapping.clone());
        state.mark_dirty(mapping.is_global());
        drop(inner);
        drop(state);
        self.finish_invalidation()
    }
}
