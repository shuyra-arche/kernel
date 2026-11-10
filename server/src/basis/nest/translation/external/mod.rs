mod state;
use super::super::address::{Layout, Placement};
use super::super::{Access, IndexNode, Nest, NestError, Span};
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
use kernel_api::address::VirtAddr;
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
    /// Empty external address organization, before any requester is admitted.
    pub(crate) fn external_empty() -> Result<Arc<Self>, NestError> {
        Arc::try_new(Self {
            id: super::super::next_id()?,
            cpu: None,
            life: Anchor::try_new().map_err(|_| NestError::OutOfMemory)?,
            mappings: super::super::authority::RangeRegistry::new(),
            layout: SpinLock::new_level(Level::Address, Layout::new()),
            pending_cleanup: SpinLock::new(None),
            state: ManuallyDrop::new(SpinLock::new_level(Level::Nest, NestState::external())),
        })
        .map_err(|_| NestError::OutOfMemory)
    }
    pub(crate) fn is_external(&self) -> bool {
        self.cpu.is_none()
    }
    pub(crate) fn external_memo(
        self: &Arc<Self>,
        address: usize,
        source: FlatRange,
        access: Access,
    ) -> Result<Arc<Span>, NestError> {
        let size = source
            .page_count()
            .checked_bytes()
            .ok_or(NestError::NotAligned)?;
        if size > 1 << 21
            || source.page_count().get() != source.grain()
            || !self.is_external()
            || address == 0
            || address >= 1usize << 38
            || !address.is_multiple_of(size)
        {
            return Err(NestError::NotAligned);
        }
        let reservation = self.reserve(Placement::fixed(VirtAddr::new(address), size)?)?;
        self.create_memo(reservation, source, access, access)
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
        let publication = external.prepare(
            memo.publication().is_some(),
            page,
            span.initial_start().get(),
            writable,
            Some((&change, memo.frozen_by(&change))),
        )?;
        memo.publish_external(expected, change, publication)
            .map_err(|_| Error::InvalidState)
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
        let memo = memos.get_mut(span, 0).map_err(|_| Error::OutOfMemory)?;
        let retracting = memo.retracting();
        match external.progress(memo.publication_mut(), retracting)? {
            state::Progress::Published => memo.finish(),
            state::Progress::Withdrawn => memo.settle_external(),
            state::Progress::Pending | state::Progress::Closed => {}
        }
        // The binding worker owns the final transition and its NestClosed wakeup.
        Ok(())
    }

    pub(crate) fn external_phase(&self) -> kernel_api::model::nest::NestPhase {
        match &self.state.lock().root {
            Storage::External(e) => e.phase(),
            _ => kernel_api::model::nest::NestPhase::Bound,
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
    pub(crate) fn delete_external_memo(&self, span: &Arc<Span>) -> Result<(), NestError> {
        let memo = {
            let mut state = self.state.lock();
            if state
                .memos
                .get(span, 0)
                .is_some_and(super::super::memo::Memo::busy)
            {
                return Err(NestError::Busy);
            }
            state.index.remove(span);
            span.state.lock().active.clear();
            state.memos.take_deleted(span)
        };
        self.give_back(&span.reservation);
        drop(memo);
        Ok(())
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

impl Nest {
    pub(crate) fn retract_external(
        &self,
        span: &Arc<Span>,
        expected: u64,
        change: Arc<Change>,
    ) -> Result<(), NestError> {
        let mut state = self.state.lock();
        let memo = state.memos.get_mut(span, 0)?;
        memo.retract_external(expected, change)
    }
}
