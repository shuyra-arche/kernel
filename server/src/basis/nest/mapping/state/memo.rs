//! A stable logical position. Hardware publication lives in Publication separately.
use crate::basis::cell::page::{Change, Freeze, Page};
use crate::basis::nest::NestError;
use alloc::sync::Arc;
#[derive(Clone, Copy)]
pub(crate) enum MemoFlight {
    Publishing,
    Retracting,
}
impl MemoFlight {
    pub(super) fn observation(self) -> kernel_api::model::cell::CellState {
        use kernel_api::model::cell::CellState;
        match self {
            Self::Publishing => CellState::Publishing,
            Self::Retracting => CellState::Retracting,
        }
    }
}
pub(crate) struct Memo {
    pub(crate) generation: crate::basis::name::Generation,
    pub(crate) page: Option<Arc<Page>>,
    /// Retraction closes admission before removing PTEs; Ready keeps this freeze.
    freeze: Option<Freeze>,
    pub(crate) flight: Option<MemoFlight>,
}
impl Memo {
    pub(crate) const fn vacant() -> Self {
        Self {
            generation: crate::basis::name::Generation::first(),
            page: None,
            freeze: None,
            flight: None,
        }
    }
    pub(crate) fn associate(&mut self, expected: u64, page: Arc<Page>) -> Result<(), NestError> {
        if self.flight.is_some() || self.generation.get() != expected {
            return Err(NestError::Busy);
        }
        let next = self.generation.next().ok_or(NestError::OutOfMemory)?;
        self.page = Some(page);
        self.freeze = None;
        self.generation = next;
        Ok(())
    }

    pub(in crate::basis::nest::mapping) fn advance(
        &mut self,
        expected: u64,
    ) -> Result<(), NestError> {
        if self.generation.get() != expected || self.flight.is_some() {
            return Err(NestError::Busy);
        }
        self.generation = self.generation.next().ok_or(NestError::OutOfMemory)?;
        Ok(())
    }

    pub(super) fn transition(
        &mut self,
        expected: u64,
        published: Option<Arc<Page>>,
        freeze: Arc<Change>,
        publishing: bool,
    ) -> Result<Arc<Page>, NestError> {
        if self.generation.get() != expected || self.flight.is_some() {
            return Err(NestError::Busy);
        }
        let page = self.page.clone().or(published).ok_or(NestError::Missing)?;
        if !Arc::ptr_eq(&page, freeze.page()) {
            return Err(NestError::Busy);
        }
        self.generation = self.generation.next().ok_or(NestError::OutOfMemory)?;
        self.page = Some(page.clone());
        self.freeze = Some(Freeze::new(freeze));
        self.flight = Some(if publishing {
            MemoFlight::Publishing
        } else {
            MemoFlight::Retracting
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
