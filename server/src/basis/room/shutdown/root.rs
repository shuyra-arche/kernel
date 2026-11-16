//! Observe the root's authoritative execution state without upgrading its Task.
//! The last temporary Task Arc can otherwise destroy a Nest under a caller's
//! COMMIT. This weak state reference grants no resource or completion authority.
use super::{LegacyLock, OnceLock, TaskState, order};
use alloc::sync::{Arc, Weak};

type State = LegacyLock<order::TaskState, TaskState>;
pub(super) struct Root {
    state: OnceLock<Weak<State>>,
}
impl Root {
    pub(super) const fn new() -> Self {
        Self {
            state: OnceLock::new(),
        }
    }
    pub(super) fn watch(&self, state: &Arc<State>) -> Result<(), ()> {
        self.state.set(Arc::downgrade(state)).map_err(|_| ())
    }
    pub(super) fn active(&self) -> bool {
        let Some(weak) = self.state.get() else {
            return false;
        };
        let Some(state) = weak.upgrade() else {
            return true;
        };
        let guard = state.lock();
        guard.is_dead()
    }
}
