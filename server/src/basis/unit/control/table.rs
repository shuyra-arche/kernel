use crate::basis::name::{Registry, Serial};
use crate::basis::unit::task::Task;
use crate::metal::sync::SpinLock;
use alloc::sync::Arc;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use kernel_api::abi::call::unit::UnitFail;
use kernel_api::model::unit::ControlId;
use kernel_api::model::unit::ControlKind;

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
struct Entry {
    kind: ControlKind,
    delegable: bool,
    authority: Arc<Authority>,
}
pub(crate) struct Table(SpinLock<Registry<ControlId, Entry>>);
impl Table {
    pub(crate) fn new() -> Self {
        Self(SpinLock::new(Registry::default()))
    }
    /// Only boot minting or checked delegation installs an entry.
    pub(crate) fn install(
        &self,
        kind: ControlKind,
        delegable: bool,
    ) -> Result<ControlId, UnitFail> {
        self.insert(kind, delegable, None)
    }
    fn insert(
        &self,
        kind: ControlKind,
        delegable: bool,
        parent: Option<Arc<Authority>>,
    ) -> Result<ControlId, UnitFail> {
        let depth = parent.as_ref().map_or(0, |parent| parent.depth + 1);
        if depth > 64 {
            return Err(UnitFail::OutOfMemory);
        }
        let authority = Arc::try_new(Authority {
            active: AtomicBool::new(true),
            parent,
            depth,
        })
        .map_err(|_| UnitFail::OutOfMemory)?;
        static NEXT: Serial = Serial::new(1, isize::MAX as usize - 1);
        let id = NEXT
            .allocate()
            .map(ControlId::new)
            .ok_or(UnitFail::OutOfMemory)?;
        let mut entries = self.0.lock();
        if entries.len() >= 1024 {
            return Err(UnitFail::OutOfMemory);
        }
        entries
            .insert(
                id,
                Entry {
                    kind,
                    delegable,
                    authority,
                },
            )
            .map_err(|_| UnitFail::OutOfMemory)?;
        Ok(id)
    }
    pub(crate) fn allows(&self, kind: ControlKind) -> bool {
        self.0
            .lock()
            .iter()
            .any(|entry| entry.kind == kind && entry.authority.alive())
    }
    pub(crate) fn matches(&self, id: ControlId, kind: ControlKind) -> bool {
        self.0
            .lock()
            .get(id)
            .is_some_and(|entry| entry.kind == kind && entry.authority.alive())
    }
    pub(crate) fn release(&self, id: ControlId) -> Result<(), UnitFail> {
        let mut entries = self.0.lock();
        entries
            .get(id)
            .ok_or(UnitFail::Denied)?
            .authority
            .active
            .store(false, Ordering::Release);
        entries.remove(id).expect("registered Control");
        Ok(())
    }
    pub(crate) fn revoke(
        &self,
        id: ControlId,
        target: &Table,
        branch: ControlId,
    ) -> Result<(), UnitFail> {
        let authority = self
            .0
            .lock()
            .get(id)
            .filter(|entry| entry.authority.alive())
            .map(|entry| entry.authority.clone())
            .ok_or(UnitFail::Denied)?;
        let entries = target.0.lock();
        let child = entries.get(branch).ok_or(UnitFail::Denied)?;
        if !child
            .authority
            .parent
            .as_ref()
            .is_some_and(|parent| Arc::ptr_eq(parent, &authority))
        {
            return Err(UnitFail::Denied);
        }
        child.authority.active.store(false, Ordering::Release);
        Ok(())
    }
    pub(crate) fn clear(&self) {
        let entries = core::mem::take(&mut *self.0.lock());
        for entry in entries.into_values() {
            entry.authority.active.store(false, Ordering::Release);
        }
    }
}
pub(crate) fn delegate(
    caller: &Arc<Task>,
    target: &Arc<Task>,
    id: ControlId,
    delegable: bool,
) -> Result<ControlId, UnitFail> {
    let _commit = crate::basis::commit();
    if !crate::basis::pend::live(caller) || !crate::basis::pend::live(target) {
        return Err(UnitFail::Denied);
    }
    if !super::adjacent(caller, target) {
        return Err(UnitFail::Denied);
    }
    let (kind, authority) = {
        let entries = caller.administration.0.lock();
        let entry = entries.get(id).ok_or(UnitFail::Denied)?;
        if !entry.delegable || !entry.authority.alive() {
            return Err(UnitFail::Denied);
        }
        (entry.kind, entry.authority.clone())
    };
    target
        .administration
        .insert(kind, delegable, Some(authority))
}
