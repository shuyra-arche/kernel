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
enum MemoFlight {
    Publishing,
    Retracting,
}
impl MemoFlight {
    fn observation(self) -> kernel_api::model::cell::CellState {
        use kernel_api::model::cell::CellState;
        match self {
            Self::Publishing => CellState::Publishing,
            Self::Retracting => CellState::Retracting,
        }
    }
}
pub(crate) struct Memo {
    generation: crate::basis::name::Generation,
    page: Option<Arc<Page>>,
    /// Retraction closes admission before removing PTEs; Ready keeps this freeze.
    freeze: Option<Freeze>,
    flight: Option<MemoFlight>,
    publication: Option<Publication>,
    controlled: bool,
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
        if self.flight.is_some() || publishing == self.publication.is_some() {
            return Err(NestError::Busy);
        }
        let flight = if publishing {
            MemoFlight::Publishing
        } else {
            MemoFlight::Retracting
        };
        self.begin(expected, freeze, flight)
    }

    fn begin(
        &mut self,
        expected: u64,
        freeze: Arc<Change>,
        flight: MemoFlight,
    ) -> Result<Arc<Page>, NestError> {
        if self.generation.get() != expected {
            return Err(NestError::Busy);
        }
        let page = self.page().ok_or(NestError::Missing)?;
        if !Arc::ptr_eq(&page, freeze.page()) {
            return Err(NestError::Busy);
        }
        let generation = self.generation.next().ok_or(NestError::OutOfMemory)?;
        let retracts = match flight {
            MemoFlight::Publishing => None,
            MemoFlight::Retracting => {
                Some(self.publication.as_ref().ok_or(NestError::Busy)?.writable())
            }
        };
        let freeze = Freeze::new(freeze, retracts)?;
        self.generation = generation;
        self.controlled = true;
        self.page = Some(page.clone());
        self.freeze = Some(freeze);
        self.flight = Some(flight);
        Ok(page)
    }

    pub(super) fn publish_external(
        &mut self,
        expected: u64,
        change: Arc<Change>,
        publication: Publication,
    ) -> Result<(), NestError> {
        if self.busy() || !Arc::ptr_eq(change.page(), publication.cell()) {
            return Err(NestError::Busy);
        }
        self.begin(expected, change, MemoFlight::Publishing)?;
        self.install(publication);
        Ok(())
    }

    pub(super) fn retract_external(
        &mut self,
        expected: u64,
        change: Arc<Change>,
    ) -> Result<(), NestError> {
        if self.publication.is_none() || self.retracting() {
            return Err(NestError::Busy);
        }
        // Explicit retraction may supersede a still-publishing leaf. All
        // validation precedes replacing its generation, freeze and flight.
        self.begin(expected, change, MemoFlight::Retracting)?;
        Ok(())
    }

    pub(super) fn retracting(&self) -> bool {
        matches!(self.flight, Some(MemoFlight::Retracting))
    }

    pub(super) fn busy(&self) -> bool {
        self.flight.is_some() || self.publication.is_some()
    }

    pub(super) fn publication(&self) -> Option<&Publication> {
        self.publication.as_ref()
    }

    pub(super) fn publication_mut(&mut self) -> Option<&mut Publication> {
        self.publication.as_mut()
    }

    fn install(&mut self, publication: Publication) {
        assert!(self.publication.is_none());
        if let Some(page) = &self.page {
            assert!(Arc::ptr_eq(page, publication.cell()));
        }
        self.publication = Some(publication);
    }

    pub(super) fn settle_external(&mut self) {
        assert!(
            self.publication()
                .is_some_and(|p| p.phase() == Phase::Settled)
        );
        self.publication = None;
        self.finish();
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
            // Device fault evidence belongs to the binding, which reports it
            // without assigning it to an inferred Memo generation.
            fault_cause: 0,
            fault_address: 0,
        }
    }
}
