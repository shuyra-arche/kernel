use kernel_api::model::identity::{PendId, TaskId, TeamId};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};

pub(crate) struct SpinLock<T>(Mutex<T>);
impl<T> SpinLock<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Mutex::new(value))
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
    pub(crate) signs: super::signs::Table,
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
            signs: super::signs::Table::new(),
            administration: super::control::Table::new(),
        })
    }
    pub(crate) fn clan(&self, _: TeamId) -> Option<()> {
        None
    }
}
#[allow(dead_code)]
pub(crate) enum WakeKey {
    SignHandle { task: TaskId, token: usize },
}
pub(crate) mod endpoint {
    use super::*;
    pub(crate) struct Endpoint {
        owner: TaskId,
        active: AtomicBool,
    }
    impl Endpoint {
        pub(crate) fn new(owner: TaskId) -> Arc<Self> {
            Arc::new(Self {
                owner,
                active: AtomicBool::new(true),
            })
        }
        pub(crate) fn alive(&self) -> bool {
            self.active.load(Ordering::Acquire)
        }
        pub(crate) fn owner(&self) -> TaskId {
            self.owner
        }
    }
    pub(crate) fn seal(endpoint: &Endpoint) {
        endpoint.active.store(false, Ordering::Release);
    }
    pub(crate) fn authority_changed(_: &Endpoint) {}
    pub(crate) fn release_actor(_: &Endpoint, _: TaskId) {}
}
