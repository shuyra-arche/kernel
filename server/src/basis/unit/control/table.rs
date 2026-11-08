use crate::basis::unit::task::Task;
use crate::machine::sync::SpinLock;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use core::sync::atomic::{AtomicBool, AtomicUsize};
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
#[derive(Clone)]
struct Entry {
    id: ControlId,
    kind: ControlKind,
    delegable: bool,
    authority: Arc<Authority>,
}
pub(crate) struct Table(SpinLock<Vec<Entry>>);
impl Table {
    pub(crate) fn new() -> Self {
        Self(SpinLock::new(Vec::new()))
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
            return Err(UnitFail::OoM);
        }
        let authority = Arc::try_new(Authority {
            active: AtomicBool::new(true),
            parent,
            depth,
        })
        .map_err(|_| UnitFail::OoM)?;
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                (id < isize::MAX as usize).then_some(id + 1)
            })
            .map(ControlId::new)
            .map_err(|_| UnitFail::OoM)?;
        let mut entries = self.0.lock();
        entries.try_reserve(1).map_err(|_| UnitFail::OoM)?;
        entries.push(Entry {
            id,
            kind,
            delegable,
            authority,
        });
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
            .iter()
            .any(|entry| entry.id == id && entry.kind == kind && entry.authority.alive())
    }
    pub(crate) fn release(&self, id: ControlId) -> Result<(), UnitFail> {
        let mut entries = self.0.lock();
        let at = entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(UnitFail::Denied)?;
        entries
            .remove(at)
            .authority
            .active
            .store(false, Ordering::Release);
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
            .iter()
            .find(|entry| entry.id == id && entry.authority.alive())
            .map(|entry| entry.authority.clone())
            .ok_or(UnitFail::Denied)?;
        let entries = target.0.lock();
        let child = entries
            .iter()
            .find(|entry| entry.id == branch)
            .ok_or(UnitFail::Denied)?;
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
        for entry in entries {
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
    let entry = caller
        .administration
        .0
        .lock()
        .iter()
        .find(|entry| entry.id == id)
        .cloned()
        .ok_or(UnitFail::Denied)?;
    if !entry.delegable || !entry.authority.alive() {
        return Err(UnitFail::Denied);
    }
    target
        .administration
        .insert(entry.kind, delegable, Some(entry.authority))
}
