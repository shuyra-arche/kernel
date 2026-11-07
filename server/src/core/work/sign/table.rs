//! Task-local event handles. Rights are independent of data borrowing.
use super::{
    WakeKey,
    endpoint::{self, Endpoint},
};
use crate::core::work::unit::task::Task;
use crate::machine::sync::SpinLock;
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use kernel_api::{
    abi::call::sign::SignFail,
    model::{
        identity::TaskId,
        sign::{SignId, SignRights},
    },
};

struct Handle {
    id: SignId,
    endpoint: Arc<Endpoint>,
    rights: SignRights,
}
#[derive(Default)]
struct Entries {
    handles: Vec<Handle>,
    owned: Vec<Arc<Endpoint>>,
}
pub(crate) struct Table(SpinLock<Entries>);
impl Table {
    pub(crate) fn new() -> Self {
        Self(SpinLock::new(Entries::default()))
    }
    /// Caller holds the work commit boundary and has checked Task admission.
    pub(crate) fn install(
        &self,
        endpoint: Arc<Endpoint>,
        rights: SignRights,
        own: bool,
    ) -> Result<SignId, SignFail> {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                (id < isize::MAX as usize).then_some(id + 1)
            })
            .map(SignId::new)
            .map_err(|_| SignFail::OoM)?;
        let mut entries = self.0.lock();
        entries.handles.try_reserve(1).map_err(|_| SignFail::OoM)?;
        if own {
            entries.owned.try_reserve(1).map_err(|_| SignFail::OoM)?;
            entries.owned.push(endpoint.clone());
        }
        entries.handles.push(Handle {
            id,
            endpoint,
            rights,
        });
        Ok(id)
    }
    pub(crate) fn get(&self, id: SignId, need: SignRights) -> Result<Arc<Endpoint>, SignFail> {
        let endpoint = {
            let entries = self.0.lock();
            let handle = entries
                .handles
                .iter()
                .find(|handle| handle.id == id)
                .ok_or(SignFail::Denied)?;
            if !handle.rights.contains(need) {
                return Err(SignFail::Denied);
            }
            handle.endpoint.clone()
        };
        if !endpoint.alive() {
            return Err(SignFail::Dead);
        }
        Ok(endpoint)
    }
    pub(crate) fn next(&self, after: usize) -> (usize, usize) {
        self.0
            .lock()
            .handles
            .iter()
            .filter(|handle| handle.id.get() > after)
            .min_by_key(|handle| handle.id.get())
            .map_or((0, 0), |handle| {
                (handle.id.get(), handle.rights.bits() as usize)
            })
    }
    pub(crate) fn close(&self, actor: TaskId, id: SignId) -> Result<(), SignFail> {
        let endpoint = self.get(id, SignRights::empty())?;
        if endpoint.owner() != actor {
            return Err(SignFail::Denied);
        }
        endpoint::seal(&endpoint);
        self.0
            .lock()
            .owned
            .retain(|owned| !Arc::ptr_eq(owned, &endpoint));
        Ok(())
    }
    pub(crate) fn release(&self, actor: TaskId, id: SignId) -> Result<(), SignFail> {
        let (handle, last) = {
            let mut entries = self.0.lock();
            let at = entries
                .handles
                .iter()
                .position(|handle| handle.id == id)
                .ok_or(SignFail::Denied)?;
            let handle = entries.handles.remove(at);
            let last = !entries
                .handles
                .iter()
                .any(|other| Arc::ptr_eq(&handle.endpoint, &other.endpoint));
            (handle, last)
        };
        if last {
            endpoint::release_actor(&handle.endpoint, actor);
        }
        super::wipe(WakeKey::SignHandle {
            task: actor,
            token: id.get(),
        });
        Ok(())
    }
    /// Owner exit closes endpoints even if its last ordinary handle was released.
    pub(crate) fn clear(&self, actor: TaskId) {
        let entries = core::mem::take(&mut *self.0.lock());
        for endpoint in entries.owned {
            endpoint::seal(&endpoint);
        }
        for handle in entries.handles {
            endpoint::release_actor(&handle.endpoint, actor);
            super::wipe(WakeKey::SignHandle {
                task: actor,
                token: handle.id.get(),
            });
        }
    }
}

pub(crate) fn delegate(
    caller: &Arc<Task>,
    target: &Arc<Task>,
    id: SignId,
    rights: SignRights,
) -> Result<SignId, SignFail> {
    let _commit = crate::core::work::commit();
    if !crate::core::work::pend::live(caller) || !crate::core::work::pend::live(target) {
        return Err(SignFail::Dead);
    }
    if !crate::core::work::unit::control::adjacent(caller, target) || rights.is_empty() {
        return Err(SignFail::Denied);
    }
    let endpoint = caller.signs.get(id, rights)?;
    target.signs.install(endpoint, rights, false)
}
