//! Effect-boundary fixture for production retirement code. It does not model
//! page tables or prove hardware completion; QEMU covers those boundaries.
use crate::{controls::Controls, retirement::Retirement};
use kernel_api::model::identity::{TaskId, TeamId};
use metal::sync::SpinLock;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) mod metal {
    #[cfg(debug_assertions)]
    pub(crate) mod allocator {
        pub(crate) struct NoAllocation;
        impl NoAllocation {
            pub(crate) fn enter() -> Self {
                Self
            }
        }
    }
    #[cfg(debug_assertions)]
    pub(crate) mod hart {
        pub(crate) struct HartId;
        impl HartId {
            pub(crate) fn get(self) -> usize {
                0
            }
        }
        pub(crate) fn hart_id() -> HartId {
            HartId
        }
    }
    pub(crate) mod stop {
        pub(crate) fn protected_stop(_: usize) {
            panic!("unexpected protected stop")
        }
    }
    pub(crate) mod sync {
        pub(crate) use std::sync::OnceLock;
        pub(crate) enum Level {
            Registry,
            L3,
        }
        pub(crate) struct SpinLock<T> {
            inner: std::sync::Mutex<T>,
            locks: core::sync::atomic::AtomicUsize,
        }
        impl<T> SpinLock<T> {
            pub(crate) const fn new(value: T) -> Self {
                Self {
                    inner: std::sync::Mutex::new(value),
                    locks: core::sync::atomic::AtomicUsize::new(0),
                }
            }
            pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, T> {
                self.locks
                    .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                self.inner.lock().unwrap()
            }
            pub(crate) const fn new_level(_: Level, value: T) -> Self {
                Self::new(value)
            }
            pub(crate) fn locks(&self) -> usize {
                self.locks.load(core::sync::atomic::Ordering::Relaxed)
            }
            pub(crate) fn try_lock(&self) -> Option<std::sync::MutexGuard<'_, T>> {
                self.inner.try_lock().ok()
            }
        }
    }
}
pub(crate) mod basis {
    // These fixtures test membership topology and reference lifetime; COMMIT,
    // interrupt exclusion and hardware effects are exercised by the RV64 guest.
    pub(crate) fn commit() {}
    pub(crate) mod unit {
        pub(crate) mod lifecycle {
            pub(crate) struct Termination;
            impl Termination {
                pub(crate) const fn new() -> Self {
                    Self
                }
            }
        }
        pub(crate) mod task {
            pub(crate) use crate::registration::{Origin, Task, TaskState, TaskWeak};
        }
        pub(crate) mod team {
            pub(crate) use crate::lifecycle::{Clearance, TeamNode};
        }
        pub(crate) mod anchor {
            pub(crate) struct Anchor;
        }
        pub(crate) mod control {
            pub(crate) enum Request {
                Pause,
                Resume,
            }
        }
    }
    pub(crate) mod sign {
        pub(crate) enum WakeKey {
            Team {
                id: kernel_api::model::identity::TeamId,
            },
        }
        pub(crate) fn signal(WakeKey::Team { id }: WakeKey) {
            let _ = id;
        }
    }
    pub(crate) mod pend {
        pub(crate) fn retire(task: &crate::Task) -> Result<(), ()> {
            task.trace.lock().push("pends");
            if task.pend_busy.load(std::sync::atomic::Ordering::Acquire) {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    pub(crate) mod nest {
        use super::super::*;
        use std::{
            collections::BTreeMap,
            sync::{Mutex, Weak},
        };
        #[derive(Debug, PartialEq, Eq)]
        pub(crate) enum NestError {
            Busy,
            OutOfMemory,
        }
        pub(crate) struct Control(pub TaskId);
        impl Control {
            pub(crate) fn control(&self) -> (TaskId, ()) {
                (self.0, ())
            }
        }
        pub(crate) struct Entry {
            pub control: Option<Control>,
            id: usize,
        }
        impl Entry {
            pub(crate) fn mapping(&self) -> Option<&usize> {
                Some(&self.id)
            }
        }
        #[derive(Default)]
        pub(crate) struct Mappings(Mutex<BTreeMap<usize, Arc<Entry>>>);
        impl Mappings {
            pub(crate) fn registered(&self, id: usize) -> Option<Arc<Entry>> {
                self.0.lock().unwrap().get(&id).cloned()
            }
        }
        pub(crate) struct Nest {
            pub mappings: Mappings,
            owner: Weak<Controls>,
            blocked: AtomicBool,
            republished: AtomicBool,
            pub attempts: SpinLock<Vec<usize>>,
        }
        impl Nest {
            pub(crate) fn new(owner: &Arc<Controls>, actor: TaskId, ids: &[usize]) -> Arc<Self> {
                let nest = Arc::new(Self {
                    mappings: Mappings::default(),
                    owner: Arc::downgrade(owner),
                    blocked: AtomicBool::new(false),
                    republished: AtomicBool::new(false),
                    attempts: SpinLock::new(Vec::new()),
                });
                for &id in ids {
                    nest.mappings.0.lock().unwrap().insert(
                        id,
                        Arc::new(Entry {
                            control: Some(Control(actor)),
                            id,
                        }),
                    );
                    owner.insert(&nest, id).unwrap();
                }
                nest
            }
            pub(crate) fn block(&self, value: bool) {
                self.blocked.store(value, Ordering::Release);
            }
            pub(crate) fn republish(&self, value: bool) {
                self.republished.store(value, Ordering::Release);
            }
            pub(crate) fn settled(&self, _: usize) -> bool {
                !self.blocked.load(Ordering::Acquire)
            }
            pub(crate) fn dispose(&self, id: usize) -> Result<(), NestError> {
                self.attempts.lock().push(id);
                Err(NestError::Busy)
            }
            pub(crate) fn retry_invalidation(&self) -> Result<(), NestError> {
                Err(NestError::Busy)
            }
            pub(crate) fn retire_mapping(&self, id: usize) {
                self.attempts.lock().push(id);
                // A competing publication wins the final COMMIT revalidation.
                if self.republished.load(Ordering::Acquire) {
                    return;
                }
                self.mappings.0.lock().unwrap().remove(&id);
                self.owner.upgrade().unwrap().remove(id);
            }
        }
    }
}
use basis::nest::NestError;
pub(crate) struct Team {
    pub id: TeamId,
    pub ready: AtomicBool,
    trace: Arc<SpinLock<Vec<&'static str>>>,
}
impl Team {
    pub(crate) fn retire(&self) -> Result<(), NestError> {
        self.trace.lock().push("team");
        if self.ready.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(NestError::Busy)
        }
    }
}
pub(crate) struct Ident {
    pub id: TaskId,
    pub busy: AtomicBool,
    trace: Arc<SpinLock<Vec<&'static str>>>,
}
impl Ident {
    pub(crate) fn receipts(&self) -> crate::lifecycle::Clearance {
        crate::lifecycle::Clearance::new(crate::topology::node(1, None))
    }
    pub(crate) fn retire(&self) -> Result<(), NestError> {
        self.trace.lock().push("memory");
        if self.busy.load(Ordering::Acquire) {
            Err(NestError::Busy)
        } else {
            Ok(())
        }
    }
}
pub(crate) struct Task {
    pub retirement: SpinLock<Retirement>,
    pub heir: SpinLock<Vec<Arc<Team>>>,
    pub controls: Arc<Controls>,
    pub ident: Ident,
    pub pend_busy: AtomicBool,
    pub preparing: AtomicBool,
    pub trace: Arc<SpinLock<Vec<&'static str>>>,
}
impl Task {
    pub(crate) fn new() -> Arc<Self> {
        let trace = Arc::new(SpinLock::new(Vec::new()));
        Arc::new(Self {
            retirement: SpinLock::new(Retirement::new()),
            heir: SpinLock::new(Vec::new()),
            controls: Arc::new(Controls::new()),
            pend_busy: AtomicBool::new(false),
            preparing: AtomicBool::new(false),
            ident: Ident {
                id: TaskId::new(1),
                busy: AtomicBool::new(false),
                trace: trace.clone(),
            },
            trace,
        })
    }
    pub(crate) fn child(&self, id: usize) -> Arc<Team> {
        let team = Arc::new(Team {
            id: TeamId::new(id),
            ready: AtomicBool::new(false),
            trace: self.trace.clone(),
        });
        self.heir.lock().push(team.clone());
        team
    }
    pub(crate) fn preparing(&self) -> bool {
        self.preparing.load(Ordering::Acquire)
    }
}
