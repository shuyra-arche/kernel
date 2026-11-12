//! Live authorization, immutable ancestry and unsettled device access.
use crate::metal::sync::{OnceLock, SpinLock};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::{
        modifiable::Modifiable,
        pend::{Distributable, Limits, Operations, PendRange, PendScope},
    },
};
mod state;
use state::{Event, Failure, Phase, State};
/// A Pend is the live access authority for one View range. Its parent obligation
/// survives handle removal until CPU withdrawal and any external use are settled.
#[derive(Clone, Copy)]
enum DeviceBorrow {
    Registers,
    External,
}
struct ParentBorrow {
    parent: Arc<Pend>,
    access: Modifiable,
    device: DeviceBorrow,
}

pub(crate) struct Pend {
    state: SpinLock<State>,
    parent: SpinLock<Option<ParentBorrow>>,
    // Immutable authority ancestry applies to every target. ParentBorrow is
    // separately the data-access obligation returned only after settlement.
    ancestor: OnceLock<Arc<Pend>>,
    holder: OnceLock<Arc<AtomicBool>>,
    // External borrows and retained object retirement both prevent settlement.
    // A retirement hold alone grants no operation and does not block narrowing.
    external: AtomicUsize,
    routing: AtomicUsize,
}

impl Pend {
    pub(crate) fn try_new_scope(
        access: Modifiable,
        scope: PendScope,
        operations: Operations,
        distributable: Distributable,
    ) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            state: SpinLock::new(State::authorized(access, scope, operations, distributable)),
            parent: SpinLock::new(None),
            ancestor: OnceLock::new(),
            holder: OnceLock::new(),
            external: AtomicUsize::new(0),
            routing: AtomicUsize::new(0),
        })
        .map_err(|_| PendFail::OutOfMemory)
    }
    pub(crate) fn scope(&self) -> PendScope {
        self.state.lock().scope
    }
    pub(crate) fn window(&self) -> Option<(PageOffset, PageCount)> {
        match self.scope().range()? {
            PendRange::Pages { offset, count } => {
                Some((PageOffset::new(offset), PageCount::new(count)?))
            }
            PendRange::Inherit => None,
        }
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
        let mut node = Some(self);
        while let Some(here) = node {
            if here
                .holder
                .get()
                .is_some_and(|closed| closed.load(Ordering::Acquire))
                || here.state.lock().phase != Phase::Active
            {
                return false;
            }
            node = here.ancestor.get().map(Arc::as_ref);
        }
        true
    }
    pub(super) fn held_by(&self, closed: &Arc<AtomicBool>) -> Result<(), PendFail> {
        if let Some(known) = self.holder.get() {
            return if Arc::ptr_eq(known, closed) {
                Ok(())
            } else {
                Err(PendFail::Denied)
            };
        }
        self.holder.set(closed.clone()).map_err(|_| PendFail::Busy)
    }
    /// Link before publication. A bounded, immutable ancestry makes logical
    /// revocation one local transition, including Sign/None/Clone branches.
    pub(super) fn inherit(&self, parent: &Arc<Self>) -> Result<(), PendFail> {
        if let Some(known) = self.ancestor.get() {
            return if Arc::ptr_eq(known, parent) {
                Ok(())
            } else {
                Err(PendFail::Denied)
            };
        }
        let mut node = Some(parent.as_ref());
        let mut depth = 1;
        while let Some(here) = node {
            if core::ptr::eq(self, here) || depth >= 64 {
                return Err(PendFail::Denied);
            }
            depth += 1;
            node = here.ancestor.get().map(Arc::as_ref);
        }
        self.ancestor
            .set(parent.clone())
            .map_err(|_| PendFail::Busy)
    }
    pub(crate) fn status(&self) -> kernel_api::model::pend::PendStatus {
        use kernel_api::model::pend::PendStatus;
        let state = *self.state.lock();
        match state.phase {
            Phase::Dead => PendStatus::Dead,
            Phase::Retiring => PendStatus::Retiring,
            Phase::Active if !self.alive() => PendStatus::Retiring,
            Phase::Active if state.usable() => PendStatus::Active,
            Phase::Active => PendStatus::Suspended,
        }
    }
    pub(crate) fn modifiable(&self) -> Option<Modifiable> {
        if !self.alive() {
            return None;
        }
        let state = *self.state.lock();
        state.usable().then_some(state.access)
    }
    pub(crate) fn can_lend_registers(&self, access: Modifiable) -> Result<(), PendFail> {
        if !self.alive() {
            return Err(PendFail::Dead);
        }
        if self.routing.load(Ordering::Acquire) != 0 {
            return Err(PendFail::Busy);
        }
        self.state
            .lock()
            .transition(Event::Borrow(access))
            .map(|_| ())
            .map_err(fail)
    }
    pub(crate) fn lend_registers(self: &Arc<Self>, child: &Arc<Self>) -> Result<(), PendFail> {
        self.borrow_device(child, DeviceBorrow::Registers)
    }
    pub(crate) fn borrow_external(self: &Arc<Self>, child: &Arc<Self>) -> Result<(), PendFail> {
        self.borrow_device(child, DeviceBorrow::External)
    }
    fn borrow_device(
        self: &Arc<Self>,
        child: &Arc<Self>,
        device: DeviceBorrow,
    ) -> Result<(), PendFail> {
        if !self.alive() {
            return Err(PendFail::Dead);
        }
        child.inherit(self)?;
        let access = child.configured();
        let mut state = self.state.lock();
        if self.routing.load(Ordering::Acquire) != 0 {
            return Err(PendFail::Busy);
        }
        *state = state.transition(Event::Borrow(access)).map_err(fail)?;
        if matches!(device, DeviceBorrow::External) {
            self.external.fetch_add(1, Ordering::Release);
        }
        *child.parent.lock() = Some(ParentBorrow {
            parent: self.clone(),
            access,
            device,
        });
        Ok(())
    }
    pub(crate) fn limits(&self) -> Limits {
        self.state.lock().limits()
    }
    pub(crate) fn permits(&self, operations: Operations) -> bool {
        self.alive() && self.state.lock().permits(operations)
    }
    pub(crate) fn can_narrow(&self, limits: Limits) -> Result<(), PendFail> {
        if !limits.valid() {
            return Err(PendFail::Denied);
        }
        let state = self.state.lock();
        state
            .transition(Event::Limit {
                access: limits.modifiable,
                operations: limits.operations,
                scope: limits.scope,
                distributable: limits.distributable,
            })
            .map(|_| ())
            .map_err(fail)
    }
    pub(crate) fn narrow(&self, limits: Limits) -> Result<(), PendFail> {
        if !limits.valid() {
            return Err(PendFail::Denied);
        }
        let mut state = self.state.lock();
        if self.routing.load(Ordering::Acquire) != 0 {
            return Err(PendFail::Busy);
        }
        *state = state
            .transition(Event::Limit {
                access: limits.modifiable,
                operations: limits.operations,
                scope: limits.scope,
                distributable: limits.distributable,
            })
            .map_err(fail)?;
        Ok(())
    }
    pub(crate) fn external_pending(&self) -> bool {
        self.external.load(Ordering::Acquire) != 0 || self.routing.load(Ordering::Acquire) != 0
    }
    /// Retain the object's retirement responsibility, not permission to keep
    /// performing an operation. Caller serializes admission with revocation.
    /// The root may still narrow; invalidation starts the object's own cleanup.
    pub(crate) fn retain_retirement(&self) -> Result<(), PendFail> {
        if !self.alive() {
            return Err(PendFail::Dead);
        }
        let state = self.state.lock();
        if !state.usable() {
            return Err(PendFail::Denied);
        }
        self.external
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_add(1)
            })
            .map(|_| ())
            .map_err(|_| PendFail::Busy)
    }
    /// # Safety
    /// The matching object was never made reachable, or all of its hardware
    /// obligations have settled. One call discharges exactly one retained hold.
    pub(crate) unsafe fn release_retirement(&self) {
        assert!(self.external.fetch_sub(1, Ordering::AcqRel) > 0);
    }
    /// Retain one live Memo publication without lending data access. Operation
    /// permission is checked by the caller against its typed target. Unlike a
    /// retirement-only hold, this prevents narrowing a permission still in use.
    /// Caller holds work commit until publication is cancelled or retained.
    pub(crate) fn retain_route(&self) -> Result<(), PendFail> {
        if !self.alive() {
            return Err(PendFail::Dead);
        }
        let state = self.state.lock();
        if !state.usable() {
            return Err(PendFail::Denied);
        }
        self.routing
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_add(1)
            })
            .map(|_| ())
            .map_err(|_| PendFail::Busy)
    }
    /// # Safety
    /// The route was never published, or its exact hardware completion settled.
    pub(crate) unsafe fn release_route(&self) {
        assert!(self.routing.fetch_sub(1, Ordering::AcqRel) > 0);
    }
    pub(crate) fn retiring(&self) -> bool {
        !self.alive()
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
    pub(super) unsafe fn cancel_unpublished(&self) -> Result<(), PendFail> {
        self.finish(Event::Cancel)
    }
    fn finish(&self, event: Event) -> Result<(), PendFail> {
        let mut state = self.state.lock();
        if self.external_pending() {
            return Err(PendFail::Busy);
        }
        *state = state.transition(event).map_err(fail)?;
        drop(state);
        if let Some(ParentBorrow {
            parent,
            access,
            device,
        }) = self.parent.lock().take()
        {
            let mut state = parent.state.lock();
            if matches!(device, DeviceBorrow::External) {
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

impl Pend {
    pub(crate) fn associates(&self) -> bool {
        self.permits(kernel_api::model::pend::Operations::Share(
            kernel_api::model::pend::ShareRights::ASSOCIATE,
        )) || self.permits(kernel_api::model::pend::Operations::Flat(
            kernel_api::model::pend::FlatRights::ASSOCIATE,
        ))
    }
}
