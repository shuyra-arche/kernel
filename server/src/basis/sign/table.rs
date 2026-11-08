//! Task-local event handles. Rights are independent of data borrowing.
use super::WakeKey;
use super::endpoint;
use super::endpoint::Endpoint;
use crate::basis::name::Registry;
use crate::basis::unit::task::Task;
use crate::machine::sync::SpinLock;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use kernel_api::abi::call::sign::SignFail;
use kernel_api::model::identity::TaskId;
use kernel_api::model::sign::SignId;
use kernel_api::model::sign::SignRights;

struct Authority {
    active: AtomicBool,
    parent: Option<Arc<Authority>>,
    depth: usize,
}
impl Authority {
    fn alive(&self) -> bool {
        let mut at = self;
        loop {
            if !at.active.load(Ordering::Acquire) {
                return false;
            }
            match &at.parent {
                Some(parent) => at = parent,
                None => return true,
            }
        }
    }
}
struct Handle {
    id: SignId,
    endpoint: Arc<Endpoint>,
    rights: SignRights,
    authority: Arc<Authority>,
}
#[derive(Default)]
struct Entries {
    handles: Registry<SignId, Handle>,
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
        self.insert(endpoint, rights, own, None)
    }
    fn insert(
        &self,
        endpoint: Arc<Endpoint>,
        rights: SignRights,
        own: bool,
        parent: Option<Arc<Authority>>,
    ) -> Result<SignId, SignFail> {
        let depth = parent.as_ref().map_or(0, |parent| parent.depth + 1);
        if depth > 64 {
            return Err(SignFail::OutOfMemory);
        }
        let authority = Arc::try_new(Authority {
            active: AtomicBool::new(true),
            parent,
            depth,
        })
        .map_err(|_| SignFail::OutOfMemory)?;
        static NEXT: crate::basis::name::Serial =
            crate::basis::name::Serial::new(1, isize::MAX as usize);
        let id = NEXT
            .allocate()
            .map(SignId::new)
            .ok_or(SignFail::OutOfMemory)?;
        let mut entries = self.0.lock();
        if own {
            entries
                .owned
                .try_reserve(1)
                .map_err(|_| SignFail::OutOfMemory)?;
        }
        entries
            .handles
            .insert(
                id,
                Handle {
                    id,
                    endpoint: endpoint.clone(),
                    rights,
                    authority,
                },
            )
            .map_err(|_| SignFail::OutOfMemory)?;
        if own {
            entries.owned.push(endpoint);
        }
        Ok(id)
    }
    pub(crate) fn get(&self, id: SignId, need: SignRights) -> Result<Arc<Endpoint>, SignFail> {
        let endpoint = {
            let entries = self.0.lock();
            let handle = entries.handles.get(id).ok_or(SignFail::Denied)?;
            if !handle.authority.alive() {
                return Err(SignFail::Dead);
            }
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
    pub(crate) fn revoke(
        &self,
        id: SignId,
        target: &Table,
        branch: SignId,
    ) -> Result<(), SignFail> {
        let authority = self.authority(id)?;
        let endpoint = {
            let entries = target.0.lock();
            let handle = entries.handles.get(branch).ok_or(SignFail::Denied)?;
            if !handle
                .authority
                .parent
                .as_ref()
                .is_some_and(|parent| Arc::ptr_eq(parent, &authority))
            {
                return Err(SignFail::Denied);
            }
            handle.authority.active.store(false, Ordering::Release);
            handle.endpoint.clone()
        };
        endpoint::authority_changed(&endpoint);
        Ok(())
    }
    fn authority(&self, id: SignId) -> Result<Arc<Authority>, SignFail> {
        let entries = self.0.lock();
        let handle = entries.handles.get(id).ok_or(SignFail::Denied)?;
        if !handle.authority.alive() {
            return Err(SignFail::Dead);
        }
        Ok(handle.authority.clone())
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
            // Close authority before releasing registration storage.
            entries
                .handles
                .get(id)
                .ok_or(SignFail::Denied)?
                .authority
                .active
                .store(false, Ordering::Release);
            let handle = entries.handles.remove(id).expect("registered Sign");
            let last = !entries.handles.iter().any(|other| {
                other.authority.alive() && Arc::ptr_eq(&handle.endpoint, &other.endpoint)
            });
            (handle, last)
        };
        endpoint::authority_changed(&handle.endpoint);
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
        for handle in entries.handles.into_values() {
            handle.authority.active.store(false, Ordering::Release);
            endpoint::authority_changed(&handle.endpoint);
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
    let _commit = crate::basis::commit();
    if !crate::basis::pend::live(caller) || !crate::basis::pend::live(target) {
        return Err(SignFail::Dead);
    }
    if !crate::basis::unit::control::adjacent(caller, target) || rights.is_empty() {
        return Err(SignFail::Denied);
    }
    let endpoint = caller.signs.get(id, rights)?;
    target
        .signs
        .insert(endpoint, rights, false, Some(caller.signs.authority(id)?))
}
