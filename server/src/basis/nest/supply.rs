//! Explicit Page publication into a pre-authorized Publication position.
use super::publication::Publication;
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
        let inner = mapping.state.lock();
        if let Some(memo) = inner.memo(page) {
            return Ok(memo.page.clone());
        }
        Ok(inner
            .published
            .iter()
            .find(|(index, _)| *index == page)
            .map(|(_, publication)| publication.cell().clone()))
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
        let mut memo =
            Publication::prepare(page, range.ceiling.contains(Access::WRITE), Some(change))?;
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

impl Nest {
    pub(crate) fn associate(
        &self,
        mapping: &Arc<Mapping>,
        index: usize,
        expected: u64,
        page: Arc<Page>,
    ) -> Result<(), NestError> {
        let state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let mut inner = mapping.state.lock();
        let address = mapping
            .initial_start()
            .get()
            .checked_add(index.checked_mul(PAGE_SIZE).ok_or(NestError::NoRegion)?)
            .ok_or(NestError::NoRegion)?;
        if inner.range_at(address).is_none() {
            return Err(NestError::NoRegion);
        }
        if inner.publishes(index) {
            return Err(NestError::AlreadyMapped);
        }
        let memo = inner.memo_mut(index)?;
        if memo.flight.is_some() {
            return Err(NestError::Busy);
        }
        memo.associate(expected, page)
    }
    pub(crate) fn inspect_memo(
        &self,
        mapping: &Arc<Mapping>,
        index: usize,
    ) -> Result<kernel_api::model::cell::CellInfo, NestError> {
        use kernel_api::model::cell::{CellInfo, CellState};
        let inner = mapping.state.lock();
        let address = mapping
            .initial_start()
            .get()
            .checked_add(index.checked_mul(PAGE_SIZE).ok_or(NestError::NoRegion)?)
            .ok_or(NestError::NoRegion)?;
        if inner.range_at(address).is_none() {
            return Err(NestError::NoRegion);
        }
        let memo = inner.memo(index);
        let publication = inner
            .published
            .iter()
            .find(|(p, _)| *p == index)
            .map(|(_, p)| p);
        let page = memo
            .and_then(|m| m.page.as_ref())
            .or_else(|| publication.map(|p| p.cell()));
        let state = memo
            .and_then(|m| m.flight)
            .unwrap_or(if publication.is_some() {
                CellState::Published
            } else if page.is_some() {
                CellState::Ready
            } else {
                CellState::Vacant
            });
        Ok(CellInfo {
            generation: memo.map_or(1, |m| m.generation),
            state,
            page: page.map_or(0, |p| p.id().get() as u64),
            frozen: page.is_some_and(|p| p.frozen()),
            external: page
                .is_some_and(|p| p.exposure() == crate::basis::cell::page::Exposure::External),
            address: VirtAddr::new(address),
            fault_cause: 0,
            fault_address: 0,
        })
    }
    /// Start a qualified logical transition; hardware operations hold no Mapping lock.
    pub(crate) fn transition_memo(
        &self,
        mapping: &Arc<Mapping>,
        index: usize,
        expected: u64,
        freeze: Arc<Change>,
        publishing: bool,
    ) -> Result<Arc<Page>, NestError> {
        use kernel_api::model::cell::CellState;
        let state = self.ready_state()?;
        self.ensure_owned(mapping, &state)?;
        let mut inner = mapping.state.lock();
        let published = inner
            .published
            .iter()
            .find(|(p, _)| *p == index)
            .map(|(_, p)| p.cell().clone());
        let memo = inner.memo_mut(index)?;
        if memo.generation != expected || memo.flight.is_some() {
            return Err(NestError::Busy);
        }
        let page = memo.page.clone().or(published).ok_or(NestError::Missing)?;
        if !Arc::ptr_eq(&page, freeze.page()) {
            return Err(NestError::Busy);
        }
        memo.generation = memo
            .generation
            .checked_add(1)
            .ok_or(NestError::OutOfMemory)?;
        memo.page = Some(page.clone());
        memo.freeze = Some(crate::basis::cell::page::Freeze::new(freeze));
        memo.flight = Some(if publishing {
            CellState::Publishing
        } else {
            CellState::Retracting
        });
        Ok(page)
    }
    /// A failed pre-publication attempt remains Ready and keeps its freeze.
    pub(crate) fn finish_memo(&self, mapping: &Arc<Mapping>, index: usize) {
        if self.has_pending_invalidation() {
            return;
        }
        let mut inner = mapping.state.lock();
        let published = inner.publishes(index);
        if let Ok(memo) = inner.memo_mut(index) {
            memo.flight = None;
            if published {
                memo.freeze = None;
            }
        }
    }
}
