//! A stable logical position. Hardware publication lives in Publication separately.
use super::super::NestError;
use crate::basis::cell::page::{Freeze, Page};
use alloc::sync::Arc;
pub(crate) struct Memo {
    pub(crate) generation: u64,
    pub(crate) page: Option<Arc<Page>>,
    /// Retraction closes admission before removing PTEs; Ready keeps this freeze.
    pub(crate) freeze: Option<Freeze>,
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
        if self.generation != expected {
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
}
