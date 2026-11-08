use super::state::Event;
use super::state::Failure;
use super::state::Phase;
use super::state::State;
use crate::machine::sync::SpinLock;
use alloc::sync::Arc;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::model::modifiable::Modifiable;

pub(crate) fn narrows(from: Modifiable, to: Modifiable) -> bool {
    from.allows(to)
}

/// A Pend is the live access authority for one View range. Its parent obligation
/// survives handle removal until CPU withdrawal and any external use are settled.
pub(crate) struct Pend {
    state: SpinLock<State>,
    window: SpinLock<Option<(PageOffset, PageCount)>>,
    parent: SpinLock<Option<(Arc<Pend>, Modifiable, bool)>>,
    external: AtomicUsize,
    routing: AtomicBool,
}
fn fail(error: Failure) -> PendFail {
    match error {
        Failure::Denied => PendFail::Denied,
        Failure::Busy => PendFail::Busy,
        Failure::Dead => PendFail::Dead,
    }
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
        .map_err(|_| PendFail::OoM)
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
    pub(crate) fn alive(&self) -> bool {
        self.state.lock().phase != Phase::Dead
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
        self.borrow_kind(child, false)
    }
    pub(crate) fn borrow_external(self: &Arc<Self>, child: &Arc<Self>) -> Result<(), PendFail> {
        self.borrow_kind(child, true)
    }
    fn borrow_kind(self: &Arc<Self>, child: &Arc<Self>, external: bool) -> Result<(), PendFail> {
        let access = child.configured();
        let mut state = self.state.lock();
        if self.routing.load(Ordering::Acquire) {
            return Err(PendFail::Busy);
        }
        *state = state.transition(Event::Borrow(access)).map_err(fail)?;
        if external {
            self.external.fetch_add(1, Ordering::Release);
        }
        *child.parent.lock() = Some((self.clone(), access, external));
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
    /// obligation until the matching IoSpace completion has settled.
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
        if self.routing.load(Ordering::Acquire) {
            return Err(PendFail::Busy);
        }
        let mut state = self.state.lock();
        *state = state.transition(Event::Settle).map_err(fail)?;
        drop(state);
        if let Some((parent, access, external)) = self.parent.lock().take() {
            let mut state = parent.state.lock();
            if external {
                parent.external.fetch_sub(1, Ordering::Release);
            }
            *state = state
                .transition(Event::Return(access))
                .expect("registered child borrow");
        }
        Ok(())
    }
}
