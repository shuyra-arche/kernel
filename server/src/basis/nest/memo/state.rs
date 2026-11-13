//! One Memo owns its logical association and exact publication/retirement state.
use super::publication::{Phase, Publication};
use super::{Publisher, publisher, source::Source};
use crate::basis::cell::page::{Change, Freeze, Page};
use crate::basis::nest::NestError;
use crate::basis::nest::authority::RangeBinding;
use crate::basis::pend::{ExternalBorrow, Pend};
use crate::basis::unit::task::memory_result::Transition;
use alloc::sync::Arc;
use kernel_api::address::VirtAddr;
use kernel_api::model::cell::{CellInfo, CellState};

pub(super) struct Association {
    /// Only the v10 adapter captures a Page. Next associations retain the
    /// logical Source alone; publications/freezes retain exact physical Pages.
    page: Option<Arc<Page>>,
    source: Source,
    binding: Arc<RangeBinding>,
}

pub(super) enum MemoPage {
    Implicit(Arc<Page>),
    Associated(Association),
}

impl MemoPage {
    fn page(&self) -> Option<&Arc<Page>> {
        match self {
            Self::Implicit(page) => Some(page),
            Self::Associated(association) => association.page.as_ref(),
        }
    }
}

/// Payload removed only after this position's hardware use has settled.
pub(in crate::basis::nest) struct Detached {
    page: Option<MemoPage>,
    freeze: Option<Freeze>,
}

impl Detached {
    pub(in crate::basis::nest) fn release(self) {
        if let Some(MemoPage::Associated(association)) = &self.page {
            let _commit = crate::basis::commit();
            association
                .source
                .source()
                .release_binding(&association.binding);
        }
        drop(self.freeze);
        drop(self.page);
    }
}

/// Completion resources leave the Nest lock before inverse owners are touched.
#[derive(Default)]
pub(in crate::basis::nest) struct Settled {
    completion: Option<(
        crate::basis::unit::task::memory_result::Completion,
        Transition,
    )>,
    publisher: Option<Publisher>,
    borrow: Option<ExternalBorrow>,
    freeze: Option<Freeze>,
    page: Option<Arc<Page>>,
    maintenance: Option<Arc<crate::basis::pend::MaintenanceUse>>,
    provenance: Option<Arc<crate::basis::pend::Provenance>>,
}

impl Settled {
    pub(in crate::basis::nest) fn release(self) {
        if let Some((completion, transition)) = self.completion {
            completion.finish(transition);
        }
        let (source_notice, publisher_notice) = {
            let _commit = crate::basis::commit();
            // SAFETY: only exact hardware completion or an unpublished cancellation
            // can construct these completion resources.
            let source_notice = self.borrow.and_then(|borrow| unsafe { borrow.settle() });
            let publisher_notice = self
                .publisher
                .map(|publisher| unsafe { publisher.settle() });
            drop(self.freeze);
            drop(self.page);
            drop(self.maintenance);
            drop(self.provenance);
            (source_notice, publisher_notice)
        };
        for (task, token) in source_notice.into_iter().chain(publisher_notice) {
            crate::basis::pend::notify(task, token);
        }
    }
}

#[derive(Clone, Copy)]
enum MemoFlight {
    Publishing,
    Retracting,
}

impl MemoFlight {
    fn observation(self) -> CellState {
        match self {
            Self::Publishing => CellState::Publishing,
            Self::Retracting => CellState::Retracting,
        }
    }
}

pub(crate) struct Memo {
    pub(in crate::basis::nest) completion:
        Option<crate::basis::unit::task::memory_result::Completion>,
    pub(in crate::basis::nest) provenance: Option<Arc<crate::basis::pend::Provenance>>,
    pub(in crate::basis::nest) maintenance: Option<Arc<crate::basis::pend::MaintenanceUse>>,
    generation: crate::basis::name::Generation,
    pub(super) page: Option<MemoPage>,
    /// Retraction closes admission before removing PTEs. The v10 Ready state
    /// keeps this freeze; logical associations release it after exact retirement.
    pub(super) freeze: Option<Freeze>,
    flight: Option<MemoFlight>,
    /// Last accepted physical access survives removal of the address policy.
    publication_access: u64,
    /// Bulk withdrawal moves the receipt into Nest's fence batch. Keep its
    /// exact physical identity here until that batch has completed.
    retiring_page: Option<Arc<Page>>,
    pub(super) publication: Option<Publication>,
    publisher: Option<Publisher>,
    borrow: Option<ExternalBorrow>,
}

impl Memo {
    pub(crate) const fn vacant() -> Self {
        Self {
            completion: None,
            maintenance: None,
            provenance: None,
            generation: crate::basis::name::Generation::first(),
            page: None,
            freeze: None,
            flight: None,
            publication_access: 0,
            retiring_page: None,
            publication: None,
            publisher: None,
            borrow: None,
        }
    }

    pub(in crate::basis::nest) fn generation(&self) -> u64 {
        self.generation.get()
    }

    pub(in crate::basis::nest) fn association(
        &self,
    ) -> Option<(Option<Arc<Page>>, Source, Arc<RangeBinding>)> {
        let MemoPage::Associated(association) = self.page.as_ref()? else {
            return None;
        };
        Some((
            association.page.clone(),
            association.source.clone(),
            association.binding.clone(),
        ))
    }

    pub(in crate::basis::nest) fn matches(&self, binding: &Arc<RangeBinding>) -> bool {
        matches!(self.page.as_ref(), Some(MemoPage::Associated(association))
            if Arc::ptr_eq(&association.binding, binding))
    }

    pub(in crate::basis::nest) fn published_by(&self, authority: &Arc<Pend>) -> bool {
        self.publisher
            .as_ref()
            .is_some_and(|publisher| Arc::ptr_eq(publisher.authority(), authority))
    }

    pub(in crate::basis::nest) fn principal(&self) -> Option<publisher::Principal> {
        self.publisher.as_ref().map(Publisher::observe)
    }

    pub(in crate::basis::nest) fn can_associate(&self, expected: u64) -> Result<(), NestError> {
        if self.busy() || self.generation.get() != expected {
            return Err(NestError::Busy);
        }
        self.generation.next().ok_or(NestError::OutOfMemory)?;
        Ok(())
    }

    pub(in crate::basis::nest) fn associate(
        &mut self,
        expected: u64,
        page: Option<Arc<Page>>,
        source: Source,
        binding: Arc<RangeBinding>,
    ) -> Detached {
        assert!(self.can_associate(expected).is_ok(), "prepared association");
        let next = self.generation.next().expect("prepared generation");
        let old = Detached {
            page: self.page.replace(MemoPage::Associated(Association {
                page,
                source,
                binding,
            })),
            freeze: self.freeze.take(),
        };
        self.generation = next;
        old
    }

    pub(in crate::basis::nest) fn remove(&mut self, expected: u64) -> Result<Detached, NestError> {
        if self.busy() || self.generation.get() != expected {
            return Err(NestError::Busy);
        }
        let next = self.generation.next().ok_or(NestError::OutOfMemory)?;
        let removed = self.clear()?;
        self.generation = next;
        Ok(removed)
    }

    /// Forced source/root cleanup is total even for an exhausted generation.
    pub(in crate::basis::nest) fn clear(&mut self) -> Result<Detached, NestError> {
        if self.busy() {
            return Err(NestError::Busy);
        }
        self.generation = self.generation.next().unwrap_or(self.generation);
        Ok(Detached {
            page: self.page.take(),
            freeze: self.freeze.take(),
        })
    }

    pub(in crate::basis::nest) fn in_flight(&self) -> bool {
        self.flight.is_some()
    }

    pub(in crate::basis::nest) fn frozen(&self) -> bool {
        self.freeze.is_some()
    }

    pub(in crate::basis::nest) fn can_advance(&self, expected: u64) -> Result<(), NestError> {
        if self.generation.get() != expected || self.flight.is_some() {
            return Err(NestError::Busy);
        }
        self.generation.next().ok_or(NestError::OutOfMemory)?;
        Ok(())
    }

    pub(in crate::basis::nest) fn advance(&mut self, expected: u64) -> Result<(), NestError> {
        self.can_advance(expected)?;
        self.generation = self.generation.next().expect("checked generation");
        Ok(())
    }

    pub(in crate::basis::nest) fn protect(&mut self, expected: u64, freeze: Option<Freeze>) {
        self.advance(expected)
            .expect("prepared protection generation");
        if let Some(freeze) = freeze {
            assert!(self.publication.is_some() && self.freeze.is_none());
            self.freeze = Some(freeze);
            self.flight = Some(MemoFlight::Publishing);
        }
    }

    pub(in crate::basis::nest) fn begin_publish(
        &mut self,
        expected: u64,
        change: Arc<Change>,
        access: u64,
    ) -> Result<(), NestError> {
        if self.busy() {
            return Err(NestError::Busy);
        }
        self.begin(expected, change, MemoFlight::Publishing, false)?;
        self.record_access(access);
        Ok(())
    }

    pub(in crate::basis::nest) fn record_access(&mut self, access: u64) {
        self.publication_access = access;
    }

    pub(in crate::basis::nest) fn retain(
        &mut self,
        publisher: Publisher,
        borrow: Option<ExternalBorrow>,
    ) {
        assert!(matches!(self.flight, Some(MemoFlight::Publishing)));
        assert!(self.publisher.is_none() && self.borrow.is_none());
        self.publisher = Some(publisher);
        self.borrow = borrow;
    }

    pub(in crate::basis::nest) fn begin_retract(
        &mut self,
        expected: u64,
        change: Arc<Change>,
        forced: bool,
    ) -> Result<(), NestError> {
        if self.publication.is_none() || self.retracting() {
            return Err(NestError::Busy);
        }
        self.begin(expected, change, MemoFlight::Retracting, forced)?;
        Ok(())
    }

    fn begin(
        &mut self,
        expected: u64,
        change: Arc<Change>,
        flight: MemoFlight,
        forced: bool,
    ) -> Result<(), NestError> {
        if self.generation.get() != expected {
            return Err(NestError::Busy);
        }
        let page = self
            .page()
            .or_else(|| {
                (self.logical() && matches!(flight, MemoFlight::Publishing))
                    .then(|| change.page().clone())
            })
            .ok_or(NestError::Missing)?;
        if !Arc::ptr_eq(&page, change.page()) {
            return Err(NestError::Busy);
        }
        let generation = match self.generation.next() {
            Some(generation) => generation,
            None if forced => self.generation,
            None => return Err(NestError::OutOfMemory),
        };
        let retracts = match flight {
            MemoFlight::Publishing => None,
            MemoFlight::Retracting if forced || self.logical() => None,
            MemoFlight::Retracting => {
                Some(self.publication.as_ref().ok_or(NestError::Busy)?.writable())
            }
        };
        let freeze = Freeze::new(change, retracts)?;
        self.generation = generation;
        if self.page.is_none() {
            self.page = Some(MemoPage::Implicit(page));
        }
        self.freeze = Some(freeze);
        self.flight = Some(flight);
        Ok(())
    }

    pub(in crate::basis::nest) fn attempted(&mut self) {
        if let Some(publisher) = &mut self.publisher {
            publisher.publish();
        }
        if let Some(borrow) = &mut self.borrow {
            borrow.publish();
        }
    }

    pub(in crate::basis::nest) fn retracting(&self) -> bool {
        matches!(self.flight, Some(MemoFlight::Retracting))
    }

    pub(in crate::basis::nest) fn busy(&self) -> bool {
        self.flight.is_some()
            || self.publication.is_some()
            || self.publisher.is_some()
            || self.borrow.is_some()
    }

    pub(in crate::basis::nest) fn logical(&self) -> bool {
        matches!(
            &self.page,
            Some(MemoPage::Associated(Association { page: None, .. }))
        )
    }

    pub(in crate::basis::nest) fn accepts_logical(&self) -> bool {
        self.page.is_none() || self.logical()
    }

    pub(in crate::basis::nest) fn publication(&self) -> Option<&Publication> {
        self.publication.as_ref()
    }

    pub(in crate::basis::nest) fn publication_mut(&mut self) -> Option<&mut Publication> {
        self.publication.as_mut()
    }

    pub(in crate::basis::nest) fn install(&mut self, publication: Publication) {
        assert!(self.publication.is_none());
        if let Some(page) = self.page() {
            assert!(Arc::ptr_eq(&page, publication.cell()));
        }
        self.publication = Some(publication);
    }

    pub(in crate::basis::nest) fn withdraw(&mut self) -> Option<Publication> {
        let publication = self.publication.take()?;
        self.retiring_page = Some(publication.cell().clone());
        if !self.retracting() {
            self.generation = self.generation.next().unwrap_or(self.generation);
            self.flight = Some(MemoFlight::Retracting);
        }
        Some(publication.retire())
    }

    pub(in crate::basis::nest) fn settle_external(&mut self) -> Settled {
        assert!(
            self.publication()
                .is_some_and(|p| p.phase() == Phase::Settled)
        );
        self.publication = None;
        self.finish()
    }

    /// Caller has the matching CPU fence or external publication/withdrawal proof.
    pub(in crate::basis::nest) fn finish(&mut self) -> Settled {
        match self.publication.as_ref().map(Publication::phase) {
            Some(Phase::Mapped) => {
                self.flight = None;
                Settled {
                    completion: self.completion.take().map(|c| (c, Transition::Published)),
                    freeze: self.freeze.take(),
                    ..Settled::default()
                }
            }
            None => {
                self.flight = None;
                let freeze = if self.logical() {
                    self.freeze.take()
                } else {
                    None
                };
                Settled {
                    completion: self.completion.take().map(|c| (c, Transition::Retired)),
                    publisher: self.publisher.take(),
                    borrow: self.borrow.take(),
                    freeze,
                    page: self.retiring_page.take(),
                    maintenance: self.maintenance.take(),
                    provenance: self.provenance.take(),
                }
            }
            _ => Settled::default(),
        }
    }

    pub(in crate::basis::nest) fn page(&self) -> Option<Arc<Page>> {
        self.page
            .as_ref()
            .and_then(|page| page.page().cloned())
            .or_else(|| {
                self.publication
                    .as_ref()
                    .map(|publication| publication.cell().clone())
            })
            .or_else(|| self.freeze.as_ref().map(|freeze| freeze.page().clone()))
            .or_else(|| self.retiring_page.clone())
    }

    pub(in crate::basis::nest) fn inspect(&self, address: VirtAddr) -> CellInfo {
        let page = self.page();
        let state = self
            .publication
            .as_ref()
            .and_then(|publication| match publication.phase() {
                Phase::Failed => Some(CellState::Failed),
                Phase::Waiting(_) | Phase::Withdrawing | Phase::Settled => {
                    Some(CellState::Retracting)
                }
                _ => None,
            })
            .or_else(|| self.flight.map(MemoFlight::observation))
            .unwrap_or(if self.publication.is_some() {
                CellState::Published
            } else if page.is_some() || self.page.is_some() {
                CellState::Ready
            } else {
                CellState::Vacant
            });
        CellInfo {
            level: page
                .as_ref()
                .map_or(kernel_api::model::cell::CellLevel::BASE, |page| {
                    page.level()
                }),
            generation: self.generation.get(),
            state,
            page: page.as_ref().map_or(0, |page| page.id().get() as u64),
            frozen: page.as_ref().is_some_and(|page| page.frozen()),
            external: page.as_ref().is_some_and(|page| {
                page.exposure() == crate::basis::cell::page::Exposure::External
            }),
            address,
            fault_cause: 0,
            fault_address: 0,
        }
    }

    pub(in crate::basis::nest) fn failed(&self, failed_fence: bool) -> bool {
        failed_fence && self.in_flight()
            || self.inspect(VirtAddr::new(0)).state == CellState::Failed
    }

    /// Observe the retained physical Page, never re-resolve the logical source.
    pub(in crate::basis::nest) fn snapshot(
        &self,
        nest: kernel_api::model::nest::NestId,
        reservation: kernel_api::model::nest::ReservationId,
        coordinate: usize,
        level: kernel_api::model::cell::CellLevel,
        failed_fence: bool,
    ) -> kernel_api::model::nest::MemoStatus {
        use kernel_api::model::nest::{MemoGeneration, MemoPhase, MemoStatus};
        let observed = self.inspect(VirtAddr::new(0));
        let source = match &self.page {
            Some(MemoPage::Associated(association)) => Some(association.source.position()),
            _ => None,
        };
        let phase = if failed_fence && self.in_flight() {
            MemoPhase::Failed
        } else {
            match observed.state {
                CellState::Publishing => MemoPhase::Publishing,
                CellState::Published => MemoPhase::Published,
                CellState::Retracting => MemoPhase::Retiring,
                CellState::Failed => MemoPhase::Failed,
                _ if source.is_some() => MemoPhase::Associated,
                _ => MemoPhase::Empty,
            }
        };
        MemoStatus {
            nest,
            reservation,
            coordinate,
            level,
            generation: MemoGeneration::new(self.generation()).expect("Memo generation"),
            phase,
            source,
            page: if self.busy() { observed.page } else { 0 },
            access: if self.busy() {
                self.publication_access
            } else {
                0
            },
        }
    }
}
