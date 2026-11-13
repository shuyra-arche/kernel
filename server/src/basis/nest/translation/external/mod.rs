mod state;
use super::super::address::Layout;
use super::super::memo::Publisher;
use super::super::{Nest, NestError, Span};
use super::{ClosePhase, NestState, Storage};
use crate::basis::cell::page::{Change, Page};
use crate::basis::nest::PublicationError as Error;
use crate::basis::pend::ExternalBorrow;
use crate::basis::unit::anchor::Anchor;
use crate::metal::iommu::{Backend, Fault};
use crate::metal::sync::{Level, SpinLock};
use alloc::sync::Arc;
use core::mem::ManuallyDrop;
pub(super) use state::External;

impl NestState {
    fn external() -> Self {
        Self {
            root: Storage::External(External::new()),
            memos: super::super::memo::Memos::new(),
            index: super::super::index::Index::new(),
            retirement: super::retirement::Retirement::new(),
            phase: ClosePhase::Open,
            team: None,
        }
    }
}
impl Nest {
    /// Empty external address organization, before any requester is admitted.
    pub(crate) fn external_empty() -> Result<Arc<Self>, NestError> {
        Arc::try_new(Self {
            id: super::super::next_id()?,
            revision: core::sync::atomic::AtomicU64::new(1),
            cpu: None,
            life: Anchor::try_new().map_err(|_| NestError::OutOfMemory)?,
            reservations: super::super::authority::ReservationRegistry::new(),
            layout: SpinLock::new_level(Level::Address, Layout::new()),
            pending_cleanup: SpinLock::new(None),
            state: ManuallyDrop::new(SpinLock::new_level(Level::Nest, NestState::external())),
        })
        .map_err(|_| NestError::OutOfMemory)
    }
    pub(crate) fn is_external(&self) -> bool {
        self.cpu.is_none()
    }
    /// Install a trusted backend once. The caller keeps it even if later preparation fails.
    pub(crate) fn bind_external(&self, hardware: Backend) -> Result<(), (Error, Backend)> {
        let mut state = self.state.lock();
        let Storage::External(external) = &mut state.root else {
            return Err((Error::InvalidState, hardware));
        };
        external.bind(hardware)
    }

    pub(crate) fn prepare_external(
        &self,
        span: &Arc<Span>,
        expected: u64,
        page: &Arc<Page>,
        change: Arc<Change>,
        writable: bool,
        publisher: &mut Option<Publisher>,
        borrow: &mut Option<ExternalBorrow>,
        maintenance: &mut Option<Arc<crate::basis::pend::MaintenanceUse>>,
        provenance: Option<Arc<crate::basis::pend::Provenance>>,
    ) -> Result<(), Error> {
        let mut state = self.state.lock();
        if !state.is_open() || !span.is_open() {
            return Err(Error::Closed);
        }
        let NestState {
            root: Storage::External(external),
            memos,
            ..
        } = &mut *state
        else {
            return Err(Error::InvalidState);
        };
        let memo = memos.get_mut(span, 0).map_err(|_| Error::OutOfMemory)?;
        let publication = if let Some(usage) = maintenance {
            external.prepare_access(
                memo.publication().is_some(),
                page,
                span.initial_start().get(),
                writable,
                Some(&change),
                Some(usage.permit()),
            )?
        } else {
            external.prepare(
                memo.publication().is_some(),
                page,
                span.initial_start().get(),
                writable,
                Some(&change),
            )?
        };
        span.prepare_revision().map_err(|_| Error::OutOfMemory)?;
        memo.begin_publish(expected, change, if writable { 6 } else { 2 })
            .map_err(|error| match error {
                NestError::OutOfMemory => Error::OutOfMemory,
                _ => Error::InvalidState,
            })?;
        span.changed();
        memo.maintenance = maintenance.take();
        memo.provenance = provenance;
        memo.retain(
            publisher.take().expect("prepared publishing route"),
            borrow.take(),
        );
        memo.install(publication);
        Ok(())
    }

    pub(crate) fn submit_external(&self, span: &Arc<Span>) -> Result<(), Error> {
        let mut state = self.state.lock();
        let NestState {
            root: Storage::External(external),
            memos,
            ..
        } = &mut *state
        else {
            return Err(Error::InvalidState);
        };
        let memo = memos.existing_mut(span, 0).ok_or(Error::UnknownBinding)?;
        memo.attempted();
        external.submit(memo.publication_mut())
    }

    pub(crate) fn close_external(&self) -> Result<(), Error> {
        let mut state = self.state.lock();
        let Storage::External(external) = &mut state.root else {
            return Err(Error::InvalidState);
        };
        external.close();
        Ok(())
    }
    /// At most one bounded backend step; the caller releases all guards before notification.
    pub(crate) fn progress_external(&self, span: &Arc<Span>) -> Result<(), Error> {
        let mut state = self.state.lock();
        let NestState {
            root: Storage::External(external),
            memos,
            ..
        } = &mut *state
        else {
            return Err(Error::InvalidState);
        };
        let Some(memo) = memos.existing_mut(span, 0) else {
            external.progress(None, false)?;
            return Ok(());
        };
        let retracting = memo.retracting();
        let before = memo.inspect(span.initial_start());
        let progress = external.progress(memo.publication_mut(), retracting);
        if progress.is_err() && memo.inspect(span.initial_start()) != before {
            span.changed();
        }
        let settled = match progress? {
            state::Progress::Published => memo.finish(),
            state::Progress::Withdrawn => memo.settle_external(),
            state::Progress::Pending | state::Progress::Closed => {
                super::super::memo::Settled::default()
            }
        };
        if memo.inspect(span.initial_start()) != before {
            span.changed();
        }
        drop(state);
        settled.release();
        // The binding worker owns the final transition and its NestClosed wakeup.
        Ok(())
    }

    pub(crate) fn external_phase(&self) -> kernel_api::model::nest::NestPhase {
        let state = self.state.lock();
        match &state.root {
            Storage::External(external) => external.phase(),
            Storage::Cpu(_) => match state.phase {
                ClosePhase::Open => kernel_api::model::nest::NestPhase::Bound,
                ClosePhase::Draining | ClosePhase::Closing => {
                    kernel_api::model::nest::NestPhase::Closing
                }
                ClosePhase::Closed => kernel_api::model::nest::NestPhase::Closed,
            },
        }
    }
    pub(crate) fn progress_external_close(&self) -> Result<bool, Error> {
        let mut state = self.state.lock();
        if !state.memos.idle() {
            return Ok(false);
        }
        match &mut state.root {
            Storage::External(e) => e.finish_close(),
            _ => Err(Error::InvalidState),
        }
    }
    pub(crate) fn delete_organization(&self) -> Result<(), NestError> {
        if !self.is_external() {
            return self.retire();
        }
        if !self.external_settled() {
            return Err(NestError::Busy);
        }
        let mut state = self.state.lock();
        if state.index.len() != 0 {
            return Err(NestError::Busy);
        }
        state.phase = ClosePhase::Closed;
        Ok(())
    }
    pub(crate) fn external_settled(&self) -> bool {
        let state = self.state.lock();
        matches!(&state.root, Storage::External(e) if e.settled())
    }
    pub(crate) fn external_error(&self) -> Option<Error> {
        let state = self.state.lock();
        match &state.root {
            Storage::External(e) => e.error(),
            _ => None,
        }
    }
    pub(crate) fn external_fault(&self) -> Result<Option<Fault>, Error> {
        let mut state = self.state.lock();
        let Storage::External(e) = &mut state.root else {
            return Err(Error::InvalidState);
        };
        e.fault()
    }
}
