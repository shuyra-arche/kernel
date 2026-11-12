//! Effect-boundary fixture for production retirement code. It does not model
//! page tables or prove hardware completion; QEMU covers those boundaries.
use crate::retirement::Retirement;
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
            // Pend retirement includes original-source withdrawal even after
            // the last Pend entry is gone. Only the fixture backend settles it.
            if task.pend_busy.load(std::sync::atomic::Ordering::Acquire)
                || task.source_busy.load(std::sync::atomic::Ordering::Acquire)
            {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    pub(crate) mod nest {
        #[derive(Debug, PartialEq, Eq)]
        pub(crate) enum NestError {
            Busy,
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
    pub node: Arc<crate::lifecycle::TeamNode>,
    pub busy: AtomicBool,
    pub retired: AtomicBool,
    trace: Arc<SpinLock<Vec<&'static str>>>,
}
impl Ident {
    pub(crate) fn receipts(&self) -> crate::lifecycle::Clearance {
        crate::lifecycle::Clearance::new(self.node.clone())
    }
    pub(crate) fn retire(&self) -> Result<(), NestError> {
        self.trace.lock().push("memory");
        if self.busy.load(Ordering::Acquire) {
            Err(NestError::Busy)
        } else {
            self.retired.store(true, Ordering::Release);
            Ok(())
        }
    }
}
pub(crate) struct Task {
    pub retirement: SpinLock<Retirement>,
    pub heir: SpinLock<Vec<Arc<Team>>>,
    pub ident: Ident,
    pub pend_busy: AtomicBool,
    pub source_busy: AtomicBool,
    pub preparing: AtomicBool,
    pub trace: Arc<SpinLock<Vec<&'static str>>>,
}
impl Task {
    pub(crate) fn new() -> Arc<Self> {
        let trace = Arc::new(SpinLock::new(Vec::new()));
        Arc::new(Self {
            retirement: SpinLock::new(Retirement::new()),
            heir: SpinLock::new(Vec::new()),
            pend_busy: AtomicBool::new(false),
            source_busy: AtomicBool::new(false),
            preparing: AtomicBool::new(false),
            ident: Ident {
                id: TaskId::new(1),
                node: crate::topology::node(1, None),
                busy: AtomicBool::new(false),
                retired: AtomicBool::new(false),
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
