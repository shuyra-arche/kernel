use crate::basis::unit::task::Task;
use crate::machine::sync::SpinLock;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use kernel_api::abi::call::unit::UnitFail;
use kernel_api::model::unit::ControlId;
use kernel_api::model::unit::ControlKind;

#[derive(Clone, Copy)]
struct Entry {
    id: ControlId,
    kind: ControlKind,
    delegable: bool,
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
        });
        Ok(id)
    }
    pub(crate) fn allows(&self, kind: ControlKind) -> bool {
        self.0.lock().iter().any(|entry| entry.kind == kind)
    }
    pub(crate) fn matches(&self, id: ControlId, kind: ControlKind) -> bool {
        self.0
            .lock()
            .iter()
            .any(|entry| entry.id == id && entry.kind == kind)
    }
    pub(crate) fn release(&self, id: ControlId) -> Result<(), UnitFail> {
        let mut entries = self.0.lock();
        let at = entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(UnitFail::Denied)?;
        entries.remove(at);
        Ok(())
    }
    pub(crate) fn clear(&self) {
        self.0.lock().clear();
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
        .copied()
        .ok_or(UnitFail::Denied)?;
    if !entry.delegable {
        return Err(UnitFail::Denied);
    }
    target.administration.install(entry.kind, delegable)
}
