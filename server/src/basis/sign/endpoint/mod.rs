pub(crate) mod delivery;
mod route;
pub(crate) mod service;
mod state;
pub(crate) mod wait;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::task::Task;
use crate::metal::sync::Level;
use crate::metal::sync::SpinLock;
use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use kernel_api::abi::call::sign::SignFail;
use kernel_api::abi::reply::SignReply;
use kernel_api::model::identity::{PendId, TaskId};
use kernel_api::model::sign::SignSource;
use state::{Event, Rejection, State};

pub(crate) use route::{Route, Watch, codec};
pub(crate) use wait::AwaitWait;

const MAX: usize = 1024;

pub struct Endpoint {
    consumed: core::sync::atomic::AtomicU64,
    state: SpinLock<State>,
    pub(crate) id: usize,
    life: Arc<Anchor>,
    routing: SpinLock<Routing>,
    owner: TaskId,
    cursor: AtomicUsize,
    delivery: SpinLock<delivery::Delivery>,
}
/// Ordinary sources and a service binding are mutually exclusive modes.
/// COMMIT spans forwarding effects; this lock owns the mode itself.
enum Routing {
    Sources(Vec<Route>),
    Service(Arc<service::Binding>),
}
impl Routing {
    fn routes(&self) -> &[Route] {
        match self {
            Self::Sources(routes) => routes,
            Self::Service(_) => &[],
        }
    }
    fn sources(&mut self) -> Result<&mut Vec<Route>, SignFail> {
        match self {
            Self::Sources(routes) => Ok(routes),
            Self::Service(_) => Err(SignFail::Denied),
        }
    }
    fn bind(&mut self, binding: Arc<service::Binding>) -> Result<(), SignFail> {
        match self {
            Self::Service(_) => Err(SignFail::Busy),
            Self::Sources(routes) if !routes.is_empty() => Err(SignFail::Denied),
            Self::Sources(_) => {
                *self = Self::Service(binding);
                Ok(())
            }
        }
    }
    fn resign(&mut self) {
        if matches!(self, Self::Service(_)) {
            *self = Self::Sources(Vec::new());
        }
    }
    fn binding(&self) -> Option<&Arc<service::Binding>> {
        match self {
            Self::Service(binding) => Some(binding),
            _ => None,
        }
    }
}
fn key(meta: &Endpoint) -> WakeKey {
    WakeKey::Sign { id: meta.id }
}
impl Endpoint {
    pub(crate) fn consumed(&self) -> u64 {
        self.consumed.load(Ordering::Acquire)
    }
    pub(crate) fn sequence(&self) -> Option<u64> {
        self.state.lock().sequence()
    }
    pub(crate) fn try_new(owner: TaskId) -> Result<Arc<Self>, crate::basis::nest::NestError> {
        Arc::try_new(Self {
            consumed: core::sync::atomic::AtomicU64::new(0),
            state: SpinLock::new_level(Level::L3, State::new()),
            id: {
                static NEXT: crate::basis::name::Serial =
                    crate::basis::name::Serial::new(1, isize::MAX as usize);
                NEXT.allocate()
                    .ok_or(crate::basis::nest::NestError::OutOfMemory)?
            },
            life: Anchor::try_new()?,
            routing: SpinLock::new_level(Level::L3, Routing::Sources(Vec::new())),
            owner,
            cursor: AtomicUsize::new(0),
            delivery: SpinLock::new(delivery::Delivery::Idle),
        })
        .map_err(|_| crate::basis::nest::NestError::OutOfMemory)
    }

    pub(crate) fn life(&self) -> Weak<Anchor> {
        Arc::downgrade(&self.life)
    }
    pub(crate) fn owner(&self) -> TaskId {
        self.owner
    }
    pub(crate) fn alive(&self) -> bool {
        self.state.lock().alive()
    }
    pub(crate) fn ready(&self) -> bool {
        self.state.lock().ready()
    }
    pub(crate) fn direct(&self) -> bool {
        self.routing.lock().routes().is_empty()
    }
    pub(crate) fn poll(&self, task: &Arc<Task>) -> Result<SignReply, SignFail> {
        if !self.alive() {
            return Err(SignFail::Dead);
        }
        if let Some(sequence) = self.state.lock().sequence() {
            return Ok(SignReply::Triggered { sequence });
        }
        let count = self.routing.lock().routes().len();
        if count == 0 {
            return Ok(SignReply::Pending);
        }
        let start = self.cursor.load(Ordering::Relaxed) % count;
        for offset in 0..count {
            let at = (start + offset) % count;
            let route = { self.routing.lock().routes().get(at).cloned() };
            let Some(route) = route else { continue };
            if route.actor != task.ident.id {
                continue;
            }
            let status = route.poll(task);
            if let Some(fail) = status {
                self.cursor.store((at + 1) % count, Ordering::Relaxed);
                return Ok(SignReply::Source {
                    source: route.watch.source(),
                    fail,
                });
            }
        }
        Ok(SignReply::Pending)
    }
}
pub(crate) fn attach(meta: &Endpoint, route: Route) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    if !meta.alive() {
        return Err(SignFail::Dead);
    }
    let mut shared = [false; 4];
    {
        let mut routing = meta.routing.lock();
        let routes = routing.sources()?;
        if routes
            .iter()
            .any(|old| old.watches(route.permit, route.watch.source()))
        {
            return Ok(());
        }
        if routes.len() >= MAX {
            return Err(SignFail::OutOfMemory);
        }
        routes.try_reserve(1).map_err(|_| SignFail::OutOfMemory)?;
        for (at, (key, _)) in route.keys.iter().enumerate() {
            shared[at] = routes.iter().any(|old| old.covers(*key));
        }
    }
    for (at, (key, life)) in route.keys.iter().enumerate() {
        if !shared[at] && sign::forward(*key, life.clone(), meta.id, meta.life()).is_err() {
            for (index, (key, _)) in route.keys.iter().take(at).enumerate() {
                if !shared[index] {
                    sign::unforward(*key, meta.id);
                }
            }
            return Err(SignFail::OutOfMemory);
        }
    }
    meta.routing.lock().sources()?.push(route);
    sign::knock(key(meta), &meta.life());
    Ok(())
}
pub(crate) fn detach(meta: &Endpoint, permit: PendId, source: SignSource) -> Result<(), SignFail> {
    detach_where(meta, permit, |known| known == source)
}

fn detach_where(
    meta: &Endpoint,
    permit: PendId,
    matches: impl Fn(SignSource) -> bool,
) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    if !meta.alive() {
        return Err(SignFail::Dead);
    }
    let route = {
        let mut routing = meta.routing.lock();
        match &mut *routing {
            Routing::Sources(routes) => routes
                .iter()
                .position(|c| c.permit == permit && matches(c.watch.source()))
                .map(|at| routes.remove(at)),
            Routing::Service(_) => None,
        }
    };
    if let Some(route) = route {
        for (key, _) in route.keys.iter() {
            let shared = meta.routing.lock().routes().iter().any(|c| c.covers(*key));
            if !shared {
                sign::unforward(*key, meta.id);
            }
        }
    }
    sign::knock(key(meta), &meta.life());
    Ok(())
}
pub(crate) fn seal(meta: &Endpoint) {
    let _commit = crate::basis::commit();
    clear(meta);
}
fn clear(meta: &Endpoint) {
    let mut state = meta.state.lock();
    *state = state.transition(Event::Close).expect("close is idempotent");
    drop(state);
    sign::wipe(trigger_key(meta));
    let routing = core::mem::replace(&mut *meta.routing.lock(), Routing::Sources(Vec::new()));
    let routes = match routing {
        Routing::Sources(routes) => routes,
        Routing::Service(_) => Vec::new(),
    };
    for route in routes {
        for (key, _) in route.keys.iter() {
            sign::unforward(*key, meta.id);
        }
    }
    sign::wipe(key(meta));
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        clear(self);
    }
}

unsafe impl Send for Endpoint {}
unsafe impl Sync for Endpoint {}

/// Source routing observes direct pending state, not another endpoint's route graph.
/// Notification forwarding therefore cannot recursively trigger route cycles.
pub(crate) fn trigger_key(meta: &Endpoint) -> WakeKey {
    WakeKey::Trigger { id: meta.id }
}

pub(crate) fn trigger(meta: &Endpoint) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    apply(meta, Event::Trigger)?;
    sign::signal(trigger_key(meta));
    sign::knock(key(meta), &meta.life());
    Ok(())
}
/// Publish retained state separately from notification propagation. A producer
/// may defer announce until outside its bounded hardware entry work.
pub(crate) fn publish(meta: &Endpoint) -> Result<u64, SignFail> {
    let _commit = crate::basis::commit();
    apply(meta, Event::Trigger)?;
    Ok(meta.sequence().expect("published event"))
}
pub(crate) fn announce(meta: &Endpoint) -> bool {
    sign::signal_batch(trigger_key(meta)) | sign::signal_batch(key(meta))
}
/// Cancellation is not successful consumption and never writes a receipt.
pub(crate) fn discard(meta: &Endpoint) {
    let _commit = crate::basis::commit();
    if let Some(sequence) = meta.sequence() {
        let _ = apply(meta, Event::Consume(sequence));
    }
}
pub(crate) fn consume(meta: &Endpoint, sequence: u64) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    apply(meta, Event::Consume(sequence))?;
    meta.consumed.store(sequence, Ordering::Release);
    Ok(())
}
fn apply(meta: &Endpoint, event: Event) -> Result<(), SignFail> {
    let mut state = meta.state.lock();
    *state = state.transition(event).map_err(|error| match error {
        Rejection::Closed => SignFail::Dead,
        Rejection::Busy => SignFail::Busy,
        Rejection::Exhausted => SignFail::OutOfMemory,
    })?;
    Ok(())
}

pub(crate) fn release_authority(meta: &Endpoint, permit: PendId) {
    let _commit = crate::basis::commit();
    while meta
        .routing
        .lock()
        .routes()
        .iter()
        .any(|route| route.permit == permit)
    {
        if detach_where(meta, permit, |_| true).is_err() {
            break;
        }
    }
}

/// Authority changes wake direct listeners and observers to revalidate their handles.
pub(crate) fn authority_changed(meta: &Endpoint) {
    sign::signal(trigger_key(meta));
    sign::signal(key(meta));
}

/// Resolve only the caller's live Pend record. SignId is descriptive and is
/// deliberately not accepted at this authorization boundary.
pub(crate) fn authorized(
    actor: &Arc<Task>,
    token: PendId,
    rights: kernel_api::model::sign::SignRights,
) -> Result<Arc<Endpoint>, SignFail> {
    use crate::basis::pend;
    let record = pend::accede(actor, token, pend::Need::Read).map_err(codec::pend_fail)?;
    if !record.permits(kernel_api::model::pend::Operations::Sign(rights)) {
        return Err(SignFail::Denied);
    }
    let endpoint = record.target().sign().ok_or(SignFail::Denied)?;
    if rights.intersects(
        kernel_api::model::sign::SignRights::CONSUME | kernel_api::model::sign::SignRights::LISTEN,
    ) && record.permits(kernel_api::model::pend::Operations::Sign(
        kernel_api::model::sign::SignRights::CONSUME,
    )) {
        crate::basis::room::interrupt::authorize(&endpoint, actor, token)?;
    }
    Ok(endpoint)
}
