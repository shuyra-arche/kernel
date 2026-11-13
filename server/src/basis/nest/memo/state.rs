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

/// Physical publication and a detached batch receipt cannot coexist. A retired
/// Page remains pinned here even when the publication moved to Nest's batch.
enum Hardware {
    Empty,
    Installed(Publication),
    Retiring(Arc<Page>),
}
/// A completion belongs to one in-flight operation. A legacy Ready position may
/// retain a freeze without an active operation; logical positions release it.
enum Activity {
    Idle(Option<Freeze>),
    Publishing {
        freeze: Freeze,
        completion: Option<crate::basis::unit::task::memory_result::Completion>,
    },
    Retracting {
        freeze: Option<Freeze>,
        completion: Option<crate::basis::unit::task::memory_result::Completion>,
    },
}
impl Activity {
    fn flight(&self) -> Option<MemoFlight> {
        match self {
            Self::Idle(_) => None,
            Self::Publishing { .. } => Some(MemoFlight::Publishing),
            Self::Retracting { .. } => Some(MemoFlight::Retracting),
        }
    }
    fn freeze(&self) -> Option<&Freeze> {
        match self {
            Self::Idle(freeze) | Self::Retracting { freeze, .. } => freeze.as_ref(),
            Self::Publishing { freeze, .. } => Some(freeze),
        }
    }
    fn completion(
        &mut self,
    ) -> Option<&mut Option<crate::basis::unit::task::memory_result::Completion>> {
        match self {
            Self::Idle(_) => None,
            Self::Publishing { completion, .. } | Self::Retracting { completion, .. } => {
                Some(completion)
            }
        }
    }
    fn take(
        &mut self,
    ) -> (
        Option<Freeze>,
        Option<crate::basis::unit::task::memory_result::Completion>,
    ) {
        match core::mem::replace(self, Self::Idle(None)) {
            Self::Idle(freeze) => (freeze, None),
            Self::Publishing { freeze, completion } => (Some(freeze), completion),
            Self::Retracting { freeze, completion } => (freeze, completion),
        }
    }
}
struct Attribution {
    publisher: Publisher,
    borrow: Option<ExternalBorrow>,
    maintenance: Option<Arc<crate::basis::pend::MaintenanceUse>>,
    provenance: Option<Arc<crate::basis::pend::Provenance>>,
}
pub(crate) struct Memo {
    generation: crate::basis::name::Generation,
    page: Option<MemoPage>,
    activity: Activity,
    publication_access: u64,
    hardware: Hardware,
    attribution: Option<Attribution>,
}
impl Memo {
    pub(crate) const fn vacant() -> Self {
        Self {
            generation: crate::basis::name::Generation::first(),
            page: None,
            activity: Activity::Idle(None),
            publication_access: 0,
            hardware: Hardware::Empty,
            attribution: None,
        }
    }
    pub(in crate::basis::nest) fn retain_completion(
        &mut self,
        completion: crate::basis::unit::task::memory_result::Completion,
    ) {
        let slot = self
            .activity
            .completion()
            .expect("completion requires an in-flight operation");
        assert!(slot.is_none());
        *slot = Some(completion);
    }
    pub(in crate::basis::nest) fn take_completion(
        &mut self,
    ) -> Option<crate::basis::unit::task::memory_result::Completion> {
        self.activity.completion().and_then(Option::take)
    }
    pub(in crate::basis::nest) fn maintenance(
        &self,
    ) -> Option<&Arc<crate::basis::pend::MaintenanceUse>> {
        self.attribution
            .as_ref()
            .and_then(|a| a.maintenance.as_ref())
    }
    pub(in crate::basis::nest) fn provenance(
        &self,
    ) -> Option<&Arc<crate::basis::pend::Provenance>> {
        self.attribution
            .as_ref()
            .and_then(|a| a.provenance.as_ref())
    }
    pub(in crate::basis::nest) fn has_resources(&self) -> bool {
        self.page.is_some() || self.frozen() || self.busy()
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
        self.attribution
            .as_ref()
            .is_some_and(|a| Arc::ptr_eq(a.publisher.authority(), authority))
    }

    pub(in crate::basis::nest) fn principal(&self) -> Option<publisher::Principal> {
        self.attribution.as_ref().map(|a| a.publisher.observe())
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
            freeze: self.activity.take().0,
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
            freeze: self.activity.take().0,
        })
    }

    pub(in crate::basis::nest) fn in_flight(&self) -> bool {
        self.activity.flight().is_some()
    }

    pub(in crate::basis::nest) fn frozen(&self) -> bool {
        self.activity.freeze().is_some()
    }

    pub(in crate::basis::nest) fn can_advance(&self, expected: u64) -> Result<(), NestError> {
        if self.generation.get() != expected || self.activity.flight().is_some() {
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
            assert!(self.publication().is_some() && self.activity.freeze().is_none());
            self.activity = Activity::Publishing {
                freeze,
                completion: None,
            };
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
        maintenance: Option<Arc<crate::basis::pend::MaintenanceUse>>,
        provenance: Option<Arc<crate::basis::pend::Provenance>>,
    ) {
        assert!(matches!(self.activity, Activity::Publishing { .. }));
        assert!(self.attribution.is_none());
        self.attribution = Some(Attribution {
            publisher,
            borrow,
            maintenance,
            provenance,
        });
    }

    pub(in crate::basis::nest) fn begin_retract(
        &mut self,
        expected: u64,
        change: Arc<Change>,
        forced: bool,
    ) -> Result<(), NestError> {
        if self.publication().is_none() || self.retracting() {
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
            MemoFlight::Retracting => Some(self.publication().ok_or(NestError::Busy)?.writable()),
        };
        let freeze = Freeze::new(change, retracts)?;
        self.generation = generation;
        if self.page.is_none() {
            self.page = Some(MemoPage::Implicit(page));
        }
        let (_, completion) = self.activity.take();
        self.activity = match flight {
            MemoFlight::Publishing => Activity::Publishing { freeze, completion },
            MemoFlight::Retracting => Activity::Retracting {
                freeze: Some(freeze),
                completion,
            },
        };
        Ok(())
    }

    pub(in crate::basis::nest) fn attempted(&mut self) {
        if let Some(attribution) = &mut self.attribution {
            attribution.publisher.publish();
            if let Some(borrow) = &mut attribution.borrow {
                borrow.publish();
            }
        }
    }

    pub(in crate::basis::nest) fn retracting(&self) -> bool {
        matches!(self.activity, Activity::Retracting { .. })
    }

    pub(in crate::basis::nest) fn busy(&self) -> bool {
        self.in_flight() || !matches!(self.hardware, Hardware::Empty) || self.attribution.is_some()
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
        match &self.hardware {
            Hardware::Installed(publication) => Some(publication),
            _ => None,
        }
    }
    pub(in crate::basis::nest) fn publication_mut(&mut self) -> Option<&mut Publication> {
        match &mut self.hardware {
            Hardware::Installed(publication) => Some(publication),
            _ => None,
        }
    }
    pub(in crate::basis::nest) fn install(&mut self, publication: Publication) {
        assert!(matches!(self.hardware, Hardware::Empty));
        if let Some(page) = self.page() {
            assert!(Arc::ptr_eq(&page, publication.cell()));
        }
        self.hardware = Hardware::Installed(publication);
    }
    /// CPU withdrawal/replacement under the Nest lock moves the old receipt
    /// straight into Nest's retirement batch before releasing that lock.
    pub(in crate::basis::nest) fn take_publication(&mut self) -> Option<Publication> {
        if !matches!(self.hardware, Hardware::Installed(_)) {
            return None;
        }
        let Hardware::Installed(publication) =
            core::mem::replace(&mut self.hardware, Hardware::Empty)
        else {
            unreachable!()
        };
        Some(publication)
    }
    pub(in crate::basis::nest) fn withdraw(&mut self) -> Option<Publication> {
        let publication = self.take_publication()?;
        self.hardware = Hardware::Retiring(publication.cell().clone());
        if !self.retracting() {
            self.generation = self.generation.next().unwrap_or(self.generation);
            let (freeze, completion) = self.activity.take();
            self.activity = Activity::Retracting { freeze, completion };
        }
        Some(publication.retire())
    }
    pub(in crate::basis::nest) fn settle_external(&mut self) -> Settled {
        assert!(
            self.publication()
                .is_some_and(|p| p.phase() == Phase::Settled)
        );
        self.hardware = Hardware::Empty;
        self.finish()
    }
    /// Caller has the matching CPU fence or external publication/withdrawal proof.
    pub(in crate::basis::nest) fn finish(&mut self) -> Settled {
        match self.publication().map(Publication::phase) {
            Some(Phase::Mapped) => {
                let (freeze, completion) = self.activity.take();
                Settled {
                    completion: completion.map(|c| (c, Transition::Published)),
                    freeze,
                    ..Settled::default()
                }
            }
            None => {
                let (freeze, completion) = self.activity.take();
                let freeze = if self.logical() {
                    freeze
                } else {
                    self.activity = Activity::Idle(freeze);
                    None
                };
                let page = match core::mem::replace(&mut self.hardware, Hardware::Empty) {
                    Hardware::Retiring(page) => Some(page),
                    Hardware::Empty => None,
                    Hardware::Installed(_) => unreachable!(),
                };
                let (publisher, borrow, maintenance, provenance) = match self.attribution.take() {
                    Some(a) => (Some(a.publisher), a.borrow, a.maintenance, a.provenance),
                    None => (None, None, None, None),
                };
                Settled {
                    completion: completion.map(|c| (c, Transition::Retired)),
                    publisher,
                    borrow,
                    freeze,
                    page,
                    maintenance,
                    provenance,
                }
            }
            _ => Settled::default(),
        }
    }
    pub(in crate::basis::nest) fn page(&self) -> Option<Arc<Page>> {
        self.page
            .as_ref()
            .and_then(|page| page.page().cloned())
            .or_else(|| match &self.hardware {
                Hardware::Empty => None,
                Hardware::Installed(publication) => Some(publication.cell().clone()),
                Hardware::Retiring(page) => Some(page.clone()),
            })
            .or_else(|| self.activity.freeze().map(|freeze| freeze.page().clone()))
    }

    pub(in crate::basis::nest) fn inspect(&self, address: VirtAddr) -> CellInfo {
        let page = self.page();
        let state = self
            .publication()
            .and_then(|publication| match publication.phase() {
                Phase::Failed => Some(CellState::Failed),
                Phase::Waiting(_) | Phase::Withdrawing | Phase::Settled => {
                    Some(CellState::Retracting)
                }
                _ => None,
            })
            .or_else(|| self.activity.flight().map(MemoFlight::observation))
            .unwrap_or(if self.publication().is_some() {
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
