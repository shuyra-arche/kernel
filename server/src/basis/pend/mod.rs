//! Authority over View ranges and the records that bind it to a Task.

mod branch;
mod ledger;
mod source;
mod state;
mod window;

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::{PageCount, PageOffset};
pub(crate) use kernel_api::model::modifiable::Modifiable;

use crate::basis::unit::task::Task;
use crate::metal::sync::SpinLock;

use state::{Event, Failure, Phase, State};

pub(crate) use branch::{accord, reduce};
pub(crate) use ledger::{
    PendEntry, Table, accede, available, changed, doom, insert, locate, notify, retire, return_,
    revoke, vestor,
};
pub(crate) use source::{ExternalBorrow, Geometry, Hold, HoldTable, SourceAuthorization};
pub(crate) use window::Window;

pub(crate) fn narrows(from: Modifiable, to: Modifiable) -> bool {
    from.allows(to)
}

/// A Pend is the live access authority for one View range. Its parent obligation
/// survives handle removal until CPU withdrawal and any external use are settled.
#[derive(Clone, Copy)]
enum BorrowKind {
    Internal,
    External,
}
struct ParentBorrow {
    parent: Arc<Pend>,
    access: Modifiable,
    kind: BorrowKind,
}

pub(crate) struct Pend {
    state: SpinLock<State>,
    window: SpinLock<Option<(PageOffset, PageCount)>>,
    parent: SpinLock<Option<ParentBorrow>>,
    external: AtomicUsize,
    routing: AtomicBool,
}

impl Pend {
    pub(crate) fn try_new_window(
        access: Modifiable,
        window: Option<(PageOffset, PageCount)>,
    ) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            state: SpinLock::new(State::new(access)),
            window: SpinLock::new(window),
            parent: SpinLock::new(None),
            external: AtomicUsize::new(0),
            routing: AtomicBool::new(false),
        })
        .map_err(|_| PendFail::OutOfMemory)
    }
    pub(crate) fn window(&self) -> Option<(PageOffset, PageCount)> {
        *self.window.lock()
    }
    pub(crate) fn contains(&self, offset: PageOffset, count: PageCount) -> bool {
        self.window().is_none_or(|(base, size)| {
            matches!((offset.get().checked_add(count.get()), base.get().checked_add(size.get())),
                (Some(end), Some(limit)) if offset.get() >= base.get() && end <= limit)
        })
    }
    pub(crate) fn configured(&self) -> Modifiable {
        self.state.lock().access
    }
    /// A retained retiring object carries obligations, but no live registration.
    pub(crate) fn alive(&self) -> bool {
        self.state.lock().phase == Phase::Active
    }
    pub(crate) fn status(&self) -> kernel_api::model::pend::PendStatus {
        use kernel_api::model::pend::PendStatus;
        let state = *self.state.lock();
        match state.phase {
            Phase::Dead => PendStatus::Dead,
            Phase::Retiring => PendStatus::Retiring,
            Phase::Active if state.usable() => PendStatus::Active,
            Phase::Active => PendStatus::Suspended,
        }
    }
    pub(crate) fn modifiable(&self) -> Option<Modifiable> {
        let state = *self.state.lock();
        state.usable().then_some(state.access)
    }
    pub(crate) fn can_borrow(&self, access: Modifiable) -> Result<(), PendFail> {
        if self.routing.load(Ordering::Acquire) {
            return Err(PendFail::Busy);
        }
        self.state
            .lock()
            .transition(Event::Borrow(access))
            .map(|_| ())
            .map_err(fail)
    }
    pub(crate) fn borrow(self: &Arc<Self>, child: &Arc<Self>) -> Result<(), PendFail> {
        self.borrow_kind(child, BorrowKind::Internal)
    }
    pub(crate) fn borrow_external(self: &Arc<Self>, child: &Arc<Self>) -> Result<(), PendFail> {
        self.borrow_kind(child, BorrowKind::External)
    }
    fn borrow_kind(self: &Arc<Self>, child: &Arc<Self>, kind: BorrowKind) -> Result<(), PendFail> {
        let access = child.configured();
        let mut state = self.state.lock();
        if self.routing.load(Ordering::Acquire) {
            return Err(PendFail::Busy);
        }
        *state = state.transition(Event::Borrow(access)).map_err(fail)?;
        if matches!(kind, BorrowKind::External) {
            self.external.fetch_add(1, Ordering::Release);
        }
        *child.parent.lock() = Some(ParentBorrow {
            parent: self.clone(),
            access,
            kind,
        });
        Ok(())
    }
    pub(crate) fn can_narrow(&self, access: Modifiable) -> Result<(), PendFail> {
        let state = self.state.lock();
        if self.routing.load(Ordering::Acquire) {
            return Err(PendFail::Busy);
        }
        state
            .transition(Event::Narrow(access))
            .map(|_| ())
            .map_err(fail)
    }
    pub(crate) fn narrow(
        &self,
        access: Modifiable,
        window: Option<(PageOffset, PageCount)>,
    ) -> Result<(), PendFail> {
        let mut state = self.state.lock();
        if self.routing.load(Ordering::Acquire) {
            return Err(PendFail::Busy);
        }
        *state = state.transition(Event::Narrow(access)).map_err(fail)?;
        *self.window.lock() = window;
        Ok(())
    }
    pub(crate) fn external_pending(&self) -> bool {
        self.external.load(Ordering::Acquire) != 0 || self.routing.load(Ordering::Acquire)
    }
    /// Retain routing responsibility without lending the BAR's CPU access.
    /// Caller holds work commit; publication must either cancel or retain this
    /// obligation until the matching Nest completion has settled.
    pub(crate) fn retain_route(&self) -> Result<(), PendFail> {
        let state = self.state.lock();
        if !state.usable() || state.access != Modifiable::Yes {
            return Err(PendFail::Denied);
        }
        self.routing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| PendFail::Busy)
    }
    /// # Safety
    /// The route was never published, or its exact hardware completion settled.
    pub(crate) unsafe fn release_route(&self) {
        assert!(self.routing.swap(false, Ordering::AcqRel));
    }
    pub(crate) fn retiring(&self) -> bool {
        self.state.lock().phase != Phase::Active
    }
    pub(crate) fn invalidate(&self) {
        let mut state = self.state.lock();
        *state = state.transition(Event::Retire).expect("retire is total");
    }
    /// Only after actual mapping withdrawal. Drop deliberately does not call this.
    pub(crate) fn settle(&self) -> Result<(), PendFail> {
        self.finish(Event::Settle)
    }
    /// # Safety
    /// This child has never authorized a published hardware access. Only the
    /// unpublished ExternalBorrow rollback owns this proof.
    pub(in crate::basis::pend) unsafe fn cancel_unpublished(&self) -> Result<(), PendFail> {
        self.finish(Event::Cancel)
    }
    fn finish(&self, event: Event) -> Result<(), PendFail> {
        let mut state = self.state.lock();
        if self.routing.load(Ordering::Acquire) {
            return Err(PendFail::Busy);
        }
        *state = state.transition(event).map_err(fail)?;
        drop(state);
        if let Some(ParentBorrow {
            parent,
            access,
            kind,
        }) = self.parent.lock().take()
        {
            let mut state = parent.state.lock();
            if matches!(kind, BorrowKind::External) {
                parent.external.fetch_sub(1, Ordering::Release);
            }
            *state = state
                .transition(Event::Return(access))
                .expect("registered child borrow");
        }
        Ok(())
    }
}

fn fail(error: Failure) -> PendFail {
    match error {
        Failure::Denied => PendFail::Denied,
        Failure::Busy => PendFail::Busy,
        Failure::Dead => PendFail::Dead,
    }
}

pub(crate) fn live(task: &Task) -> bool {
    !task.dead()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Need {
    Read,
    Write,
}
impl Need {
    pub(crate) fn allowed_by(self, access: Modifiable) -> bool {
        match self {
            Self::Write => access == Modifiable::Yes,
            Self::Read => true,
        }
    }
}
