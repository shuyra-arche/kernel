//! A stable logical position and its hardware publication. Nest owns this record.
mod table;
pub(super) use table::Memos;
pub(crate) mod publication;
use crate::basis::cell::page::{Change, Freeze, Page};
use crate::basis::nest::NestError;
use alloc::sync::Arc;
use kernel_api::address::VirtAddr;
use kernel_api::model::cell::{CellInfo, CellState};
use publication::{Phase, Publication};
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
    pub(in crate::basis::nest) freeze: Option<Freeze>,
    pub(crate) flight: Option<MemoFlight>,
    pub(in crate::basis::nest) publication: Option<Publication>,
    controlled: bool,
    pub(in crate::basis::nest) fault: Option<crate::metal::generic::iommu::Fault>,
}
impl Memo {
    pub(crate) const fn vacant() -> Self {
        Self {
            generation: crate::basis::name::Generation::first(),
            page: None,
            freeze: None,
            flight: None,
            publication: None,
            controlled: false,
            fault: None,
        }
    }
    pub(crate) fn associate(&mut self, expected: u64, page: Arc<Page>) -> Result<(), NestError> {
        if self.flight.is_some() || self.publication.is_some() || self.generation.get() != expected
        {
            return Err(NestError::Busy);
        }
        let next = self.generation.next().ok_or(NestError::OutOfMemory)?;
        self.controlled = true;
        self.page = Some(page);
        self.freeze = None;
        self.generation = next;
        Ok(())
    }

    pub(in crate::basis::nest) fn advance(&mut self, expected: u64) -> Result<(), NestError> {
        if self.generation.get() != expected || self.flight.is_some() {
            return Err(NestError::Busy);
        }
        self.generation = self.generation.next().ok_or(NestError::OutOfMemory)?;
        self.controlled = true;
        Ok(())
    }

    pub(super) fn transition(
        &mut self,
        expected: u64,
        freeze: Arc<Change>,
        publishing: bool,
    ) -> Result<Arc<Page>, NestError> {
        if self.generation.get() != expected || self.flight.is_some() {
            return Err(NestError::Busy);
        }
        let page = self.page().ok_or(NestError::Missing)?;
        if !Arc::ptr_eq(&page, freeze.page()) {
            return Err(NestError::Busy);
        }
        self.generation = self.generation.next().ok_or(NestError::OutOfMemory)?;
        self.controlled = true;
        self.page = Some(page.clone());
        self.freeze = Some(Freeze::new(freeze));
        self.flight = Some(if publishing {
            MemoFlight::Publishing
        } else {
            MemoFlight::Retracting
        });
        Ok(page)
    }

    pub(super) fn finish(&mut self) {
        let published = self.publication.is_some();
        self.flight = None;
        if published {
            self.freeze = None;
        }
    }
}

impl Memo {
    pub(super) fn page(&self) -> Option<Arc<Page>> {
        self.page
            .clone()
            .or_else(|| self.publication.as_ref().map(|p| p.cell().clone()))
    }

    pub(super) fn inspect(&self, address: VirtAddr) -> CellInfo {
        let page = self
            .page
            .as_ref()
            .or_else(|| self.publication.as_ref().map(|p| p.cell()));
        let state = self
            .publication
            .as_ref()
            .and_then(|p| match p.phase() {
                Phase::Failed => Some(CellState::Failed),
                Phase::Waiting(_) | Phase::Withdrawing | Phase::Settled => {
                    Some(CellState::Retracting)
                }
                _ => None,
            })
            .or_else(|| self.flight.map(|f| f.observation()))
            .unwrap_or(if self.publication.is_some() {
                CellState::Published
            } else if page.is_some() {
                CellState::Ready
            } else {
                CellState::Vacant
            });
        CellInfo {
            level: page.map_or(kernel_api::model::cell::CellLevel::BASE, |p| p.level()),
            generation: self.generation.get(),
            state,
            page: page.map_or(0, |p| p.id().get() as u64),
            frozen: page.is_some_and(|p| p.frozen()),
            external: page
                .is_some_and(|p| p.exposure() == crate::basis::cell::page::Exposure::External),
            address,
            fault_cause: self.fault.map_or(0, |f| f.cause as u64),
            fault_address: self.fault.map_or(0, |f| f.address),
        }
    }
}
