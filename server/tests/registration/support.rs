use kernel_api::model::identity::{PendId, TaskId, TeamId};
use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) struct SpinLock<T>(Mutex<T>);
impl<T> SpinLock<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }
    pub(crate) fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.0.try_lock().ok()
    }
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
pub(crate) mod spin {
    pub(crate) type SpinLockGuard<'a, T> = std::sync::MutexGuard<'a, T>;
}
pub(crate) struct HoldTable;
impl HoldTable {
    pub(crate) fn new() -> Self {
        Self
    }
}
pub(crate) struct PendEntry(pub(crate) PendId);
impl PendEntry {
    pub(crate) fn authority(&self) -> &Self {
        self
    }
    pub(crate) fn held_by(
        &self,
        _: &Arc<core::sync::atomic::AtomicBool>,
    ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        Ok(())
    }
    pub(crate) fn token(&self) -> PendId {
        self.0
    }
}

pub(crate) struct Team {
    pub(crate) id: TeamId,
    pub(crate) creator: TaskId,
}
pub(crate) struct Ident {
    pub(crate) id: TaskId,
    pub(crate) controller: TaskId,
    pub(crate) team: Team,
}
pub(crate) struct Task {
    pub(crate) ident: Ident,
    pub(crate) administration: super::control::Table,
}
impl Task {
    pub(crate) fn new(id: usize) -> Arc<Self> {
        Arc::new(Self {
            ident: Ident {
                id: TaskId::new(id),
                controller: TaskId::new(0),
                team: Team {
                    id: TeamId::new(1),
                    creator: TaskId::new(0),
                },
            },
            administration: super::control::Table::new(),
        })
    }
    pub(crate) fn clan(&self, _: TeamId) -> Option<()> {
        None
    }
}
