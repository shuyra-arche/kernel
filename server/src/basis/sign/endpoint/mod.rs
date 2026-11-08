mod route;
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
use kernel_api::model::identity::TaskId;
use kernel_api::model::sign::SignSource;
use state::{Event, Rejection, State};

pub(crate) use route::{Route, Watch, codec};
pub(crate) use wait::AwaitWait;

const MAX: usize = 1024;

pub struct Endpoint {
    state: SpinLock<State>,
    pub(crate) id: usize,
    life: Arc<Anchor>,
    routes: SpinLock<Vec<Route>>,
    owner: TaskId,
    cursor: AtomicUsize,
}
fn key(meta: &Endpoint) -> WakeKey {
    WakeKey::Sign { id: meta.id }
}
impl Endpoint {
    pub(crate) fn try_new(owner: TaskId) -> Result<Arc<Self>, crate::basis::nest::NestError> {
        Arc::try_new(Self {
            state: SpinLock::new_level(Level::L3, State::new()),
            id: {
                static NEXT: crate::basis::name::Serial =
                    crate::basis::name::Serial::new(1, isize::MAX as usize);
                NEXT.allocate()
                    .ok_or(crate::basis::nest::NestError::OutOfMemory)?
            },
            life: Anchor::try_new()?,
            routes: SpinLock::new_level(Level::L3, Vec::new()),
            owner,
            cursor: AtomicUsize::new(0),
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
    pub(crate) fn poll(&self, task: &Arc<Task>) -> Result<SignReply, SignFail> {
        if !self.alive() {
            return Err(SignFail::Dead);
        }
        if let Some(sequence) = self.state.lock().sequence() {
            return Ok(SignReply::Triggered { sequence });
        }
        let count = self.routes.lock().len();
        if count == 0 {
            return Ok(SignReply::Pending);
        }
        let start = self.cursor.load(Ordering::Relaxed) % count;
        for offset in 0..count {
            let at = (start + offset) % count;
            let route = { self.routes.lock().get(at).cloned() };
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
        let mut routes = meta.routes.lock();
        if routes
            .iter()
            .any(|old| old.watches(route.actor, route.watch.source()))
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
    meta.routes.lock().push(route);
    sign::knock(key(meta), &meta.life());
    Ok(())
}
pub(crate) fn detach(meta: &Endpoint, actor: TaskId, source: SignSource) -> Result<(), SignFail> {
    detach_where(meta, actor, |known| known == source)
}

fn detach_where(
    meta: &Endpoint,
    actor: TaskId,
    matches: impl Fn(SignSource) -> bool,
) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    if !meta.alive() {
        return Err(SignFail::Dead);
    }
    let route = {
        let mut routes = meta.routes.lock();
        routes
            .iter()
            .position(|c| c.actor == actor && matches(c.watch.source()))
            .map(|at| routes.remove(at))
    };
    if let Some(route) = route {
        for (key, _) in route.keys.iter() {
            let shared = meta.routes.lock().iter().any(|c| c.covers(*key));
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
    let routes = core::mem::take(&mut *meta.routes.lock());
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
pub(crate) fn consume(meta: &Endpoint, sequence: u64) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    apply(meta, Event::Consume(sequence))
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

pub(crate) fn release_actor(meta: &Endpoint, actor: TaskId) {
    while meta.routes.lock().iter().any(|route| route.actor == actor) {
        if detach_where(meta, actor, |_| true).is_err() {
            break;
        }
    }
}

/// Authority changes wake direct listeners and observers to revalidate their handles.
pub(crate) fn authority_changed(meta: &Endpoint) {
    sign::signal(trigger_key(meta));
    sign::signal(key(meta));
}
