//! Effect boundary for the production Site/Chain storage contract.
use crate::chain::Link;
use std::sync::{Arc, Mutex, MutexGuard};
pub(crate) struct Task {
    pub state: Lock<State>,
}
pub(crate) struct State(Link);
impl State {
    pub(crate) fn link(&mut self) -> &mut Link {
        &mut self.0
    }
}
impl Task {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Lock::new(State(Link::Detached)),
        })
    }
    pub(crate) fn waiting(&self) -> bool {
        true
    }
}
pub(crate) struct Lock<T>(Mutex<T>);
impl<T> Lock<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }
    pub(crate) fn new_level(_: Level, value: T) -> Self {
        Self::new(value)
    }
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
pub(crate) enum Level {
    L3,
}
pub(crate) struct Anchor;
impl Anchor {
    pub(crate) fn dead(life: &std::sync::Weak<Self>) -> bool {
        life.strong_count() == 0
    }
}
pub(crate) mod metal {
    pub(crate) mod sync {
        pub(crate) use super::super::{Level, Lock as SpinLock};
        pub(crate) use std::sync::OnceLock;
    }
}
pub(crate) mod basis {
    pub(crate) mod nest {
        pub(crate) use kernel_api::model::identity::TeamId as NestId;
    }
    pub(crate) mod unit {
        pub(crate) mod anchor {
            pub(crate) use crate::support::Anchor;
        }
        pub(crate) mod task {
            pub(crate) use crate::Task;
            pub(crate) mod chain {
                pub(crate) use crate::chain::Chain;
            }
        }
    }
}
