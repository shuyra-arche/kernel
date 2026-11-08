//! A stable logical position. Hardware publication lives in Publication separately.
use crate::basis::cell::page::{Change, Freeze, Page};
use crate::basis::nest::NestError;
use alloc::sync::Arc;
pub(crate) struct Memo {
    pub(crate) generation: u64,
    pub(crate) page: Option<Arc<Page>>,
    /// Retraction closes admission before removing PTEs; Ready keeps this freeze.
    freeze: Option<Freeze>,
    pub(crate) flight: Option<kernel_api::model::cell::CellState>,
}
impl Memo {
    pub(crate) const fn vacant() -> Self {
        Self {
            generation: 1,
            page: None,
            freeze: None,
            flight: None,
        }
    }
    pub(crate) fn associate(&mut self, expected: u64, page: Arc<Page>) -> Result<(), NestError> {
        if self.flight.is_some() || self.generation != expected {
            return Err(NestError::Busy);
        }
        let next = self
            .generation
            .checked_add(1)
            .ok_or(NestError::OutOfMemory)?;
        self.page = Some(page);
        self.freeze = None;
        self.generation = next;
        Ok(())
    }

    pub(in crate::basis::nest::mapping) fn advance(
        &mut self,
        expected: u64,
    ) -> Result<(), NestError> {
        if self.generation != expected || self.flight.is_some() {
            return Err(NestError::Busy);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(NestError::OutOfMemory)?;
        Ok(())
    }

    pub(super) fn transition(
        &mut self,
        expected: u64,
        published: Option<Arc<Page>>,
        freeze: Arc<Change>,
        publishing: bool,
    ) -> Result<Arc<Page>, NestError> {
        use kernel_api::model::cell::CellState;
        if self.generation != expected || self.flight.is_some() {
            return Err(NestError::Busy);
        }
        let page = self.page.clone().or(published).ok_or(NestError::Missing)?;
        if !Arc::ptr_eq(&page, freeze.page()) {
            return Err(NestError::Busy);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(NestError::OutOfMemory)?;
        self.page = Some(page.clone());
        self.freeze = Some(Freeze::new(freeze));
        self.flight = Some(if publishing {
            CellState::Publishing
        } else {
            CellState::Retracting
        });
        Ok(page)
    }

    pub(super) fn finish(&mut self, published: bool) {
        self.flight = None;
        if published {
            self.freeze = None;
        }
    }
}
