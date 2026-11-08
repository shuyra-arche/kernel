mod state;
use super::super::address::{Layout, Placement};
use super::super::{Access, IndexNode, Nest, NestError, NestId, Span};
use super::{ClosePhase, NestState, Storage};
use crate::basis::cell::page::{Change, Page};
use crate::basis::flat::FlatRange;
use crate::basis::nest::PublicationError as Error;
use crate::basis::unit::anchor::Anchor;
use crate::metal::iommu::{Backend, Fault};
use crate::metal::sync::{Level, SpinLock};
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::mem::ManuallyDrop;
use kernel_api::address::{PAGE_SIZE, VirtAddr};
pub(super) use state::External;

impl NestState {
    fn external() -> Self {
        Self {
            root: Storage::External(External::new()),
            memos: super::super::memo::Memos::new(),
            index: super::super::index::Index::new(),
            retirement: super::retirement::Retirement::new(),
            phase: ClosePhase::Open,
        }
    }
}
impl Nest {
    /// The existing one-shot route has a checked, single-page IOVA contract.
    pub(crate) fn external(
        address: usize,
        source: FlatRange,
        access: Access,
    ) -> Result<(Arc<Self>, Arc<Span>), NestError> {
        if address == 0 || address >= 1usize << 38 || !address.is_multiple_of(PAGE_SIZE) {
            return Err(NestError::NotAligned);
        }
        let nest = Arc::try_new(Self {
            id: NestId::allocate()?,
            cpu: None,
            life: Anchor::try_new().map_err(|_| NestError::OutOfMemory)?,
            mappings: super::super::authority::RangeRegistry::new(),
            layout: SpinLock::new_level(Level::Address, Layout::new()),
            pending_cleanup: SpinLock::new(None),
            state: ManuallyDrop::new(SpinLock::new_level(Level::Nest, NestState::external())),
        })
        .map_err(|_| NestError::OutOfMemory)?;
        let reservation = nest.reserve(Placement::fixed(VirtAddr::new(address), PAGE_SIZE)?)?;
        let span = nest.create_memo(reservation, source, access, access)?;
        Ok((nest, span))
    }
    pub(in crate::basis::nest) fn commit_external(
        &self,
        span: &Arc<Span>,
    ) -> Result<(), NestError> {
        let mut state = self.state.lock();
        let Storage::External(external) = &state.root else {
            return Err(NestError::NoRegion);
        };
        if !external.open() || !span.state.lock().explicit {
            return Err(NestError::Busy);
        }
        let node =
            Box::try_new(IndexNode::new(span.clone())?).map_err(|_| NestError::OutOfMemory)?;
        state.memos.get_mut(span, 0)?;
        state.index.insert(node);
        Ok(())
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
    ) -> Result<(), Error> {
        let mut state = self.state.lock();
        let NestState {
            root: Storage::External(external),
            memos,
            ..
        } = &mut *state
        else {
            return Err(Error::InvalidState);
        };
        let memo = memos.get_mut(span, 0).map_err(|_| Error::OutOfMemory)?;
        external.prepare(
            &mut memo.publication,
            page,
            span.initial_start().get(),
            writable,
            Some(&change),
        )?;
        if let Err(_) = memo.transition(expected, change, true) {
            memo.publication = None; // no hardware effects during preparation
            return Err(Error::InvalidState);
        }
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
        let memo = memos.get_mut(span, 0).map_err(|_| Error::OutOfMemory)?;
        external.submit(&mut memo.publication)
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
    pub(crate) fn progress_external(&self, span: &Arc<Span>) -> Result<bool, Error> {
        let mut state = self.state.lock();
        let NestState {
            root: Storage::External(external),
            memos,
            ..
        } = &mut *state
        else {
            return Err(Error::InvalidState);
        };
        let memo = memos.get_mut(span, 0).map_err(|_| Error::OutOfMemory)?;
        match external.progress(&mut memo.publication)? {
            state::Progress::Published | state::Progress::Withdrawn => memo.finish(),
            state::Progress::Pending | state::Progress::Closed => {}
        }
        Ok(external.settled())
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

impl Nest {
    pub(crate) fn record_fault(&self, span: &Arc<Span>, fault: Fault) {
        if let Ok(memo) = self.state.lock().memos.get_mut(span, 0) {
            memo.fault.get_or_insert(fault);
        }
    }
    pub(crate) fn retract_external(
        &self,
        span: &Arc<Span>,
        expected: u64,
        change: Arc<Change>,
    ) -> Result<(), NestError> {
        let mut state = self.state.lock();
        let memo = state.memos.get_mut(span, 0)?;
        // A publication can still be in flight when its owner explicitly retracts.
        // Check the public generation before replacing that flight with withdrawal.
        if memo.generation.get() != expected {
            return Err(NestError::Busy);
        }
        // A failed replacement must preserve the previous in-flight state.
        let previous = memo.flight.take();
        if let Err(error) = memo.transition(expected, change, false) {
            memo.flight = previous;
            return Err(error);
        }
        Ok(())
    }
}
