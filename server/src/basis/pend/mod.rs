//! Authority over View ranges and the records that bind it to a Task.

mod accord;
mod cleanup;
mod entry;
mod external;
mod geometry;
mod hold;
mod ledger;
mod narrow;
mod need;
mod source;
mod state;
mod window;

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::{PageCount, PageOffset};
use kernel_api::model::identity::PendId;
pub(crate) use kernel_api::model::modifiable::Modifiable;

use crate::basis::unit::task::Task;
use crate::machine::sync::SpinLock;

use state::{Event, Failure, Phase, State};

pub(crate) use accord::accord;
pub(crate) use cleanup::{doom, retire, return_, revoke};
pub(crate) use entry::{PendEntry, PendSnapshot, accede, locate, vestor};
pub(crate) use external::ExternalBorrow;
pub(crate) use geometry::Geometry;
pub(crate) use hold::{Hold, HoldTable};
pub(crate) use ledger::Table;
pub(crate) use narrow::reduce;
pub(crate) use need::Need;
pub(crate) use source::SourceAuthorization;
pub(crate) use window::Window;

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

fn fail(error: Failure) -> PendFail {
    match error {
        Failure::Denied => PendFail::Denied,
        Failure::Busy => PendFail::Busy,
        Failure::Dead => PendFail::Dead,
    }
}

pub(super) const RETRIES: usize = 8;

pub(crate) fn changed(task: &Task) {
    task.pend.version.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn live(task: &Task) -> bool {
    !task.dead()
}

pub(super) fn with_pair<T>(a: &Task, b: &Task, run: impl FnOnce() -> T) -> T {
    with_pair_uncommitted(a, b, || {
        let _commit = crate::basis::commit();
        run()
    })
}

pub(super) fn with_pair_uncommitted<T>(a: &Task, b: &Task, run: impl FnOnce() -> T) -> T {
    if a.ident.id == b.ident.id {
        let _guard = a.pend.lock();
        run()
    } else if a.ident.id.get() < b.ident.id.get() {
        let _first = a.pend.lock();
        let _second = b.pend.lock();
        run()
    } else {
        let _first = b.pend.lock();
        let _second = a.pend.lock();
        run()
    }
}

pub(super) fn remember(
    tasks: &mut Vec<(Arc<Task>, usize)>,
    task: &Arc<Task>,
    version: usize,
) -> Result<(), PendFail> {
    if let Some((_, old)) = tasks.iter().find(|(old, _)| Arc::ptr_eq(old, task)) {
        return if *old == version {
            Ok(())
        } else {
            Err(PendFail::Busy)
        };
    }
    tasks.try_reserve(1).map_err(|_| PendFail::OoM)?;
    tasks.push((task.clone(), version));
    Ok(())
}

pub(super) fn observe(
    tasks: &mut Vec<(Arc<Task>, usize)>,
    task: &Arc<Task>,
    version: usize,
) -> Result<(), PendFail> {
    if tasks.iter().any(|(old, _)| Arc::ptr_eq(old, task)) {
        return Ok(());
    }
    remember(tasks, task, version)
}

pub(crate) fn available(pend_entry: &PendSnapshot) -> Result<(), PendFail> {
    if pend_entry.authority().modifiable().is_none() {
        return Err(PendFail::HandedOver);
    }
    Ok(())
}

pub(super) fn with_tasks_checked<T>(
    tasks: &[(Arc<Task>, usize)],
    validate: impl FnOnce() -> bool,
    run: impl FnOnce() -> T,
) -> Result<T, PendFail> {
    crate::machine::sync::reserve_depend(tasks.len() + 8).map_err(|_| PendFail::OoM)?;
    debug_assert!(
        tasks
            .windows(2)
            .all(|pair| pair[0].0.ident.id.get() < pair[1].0.ident.id.get())
    );
    let mut guards = Vec::new();
    guards.try_reserve(tasks.len()).map_err(|_| PendFail::OoM)?;
    for (task, _) in tasks {
        guards.push(task.pend.lock());
    }
    let commit = crate::basis::commit();
    let valid = tasks
        .iter()
        .all(|(task, version)| task.pend.version.load(Ordering::Relaxed) == *version)
        || validate();
    let result = if valid {
        Ok(run())
    } else {
        Err(PendFail::Busy)
    };
    drop(commit);
    while guards.pop().is_some() {}
    result
}

pub(super) fn insert_heir(task: &Task, parent: PendId, child: Weak<Task>, token: PendId) {
    let mut heirs = task.pend.heirs.lock();
    let at = heirs.partition_point(|(source, _, _)| source.get() <= parent.get());
    heirs.insert(at, (parent, child, token));
}

pub(super) fn remove_heir(task: &Task, parent: PendId, token: PendId) {
    let mut heirs = task.pend.heirs.lock();
    let start = heirs.partition_point(|(source, _, _)| source.get() < parent.get());
    if let Some(at) = heirs[start..]
        .iter()
        .take_while(|(source, _, _)| *source == parent)
        .position(|(_, _, child)| *child == token)
    {
        heirs.remove(start + at);
    }
}

pub(crate) fn insert(task: &Task, pend_entry: PendEntry) -> Result<(), PendFail> {
    let _closed = task.pend.lock();
    let _commit = crate::basis::commit();
    if !live(task) {
        return Err(PendFail::Dead);
    }
    let mut entries = task.pend.entries.lock();
    entries.try_reserve(1).map_err(|_| PendFail::OoM)?;
    entries.push(pend_entry);
    changed(task);
    Ok(())
}

pub(crate) fn notify(task: kernel_api::model::identity::TaskId, token: PendId) {
    let _ = crate::basis::sign::signal(crate::basis::sign::WakeKey::Inspect {
        task,
        token: token.get(),
    });
}
