//! Production registry and Weak ownership, with a minimal Task payload. The
//! destructor invokes the production removal at the same last-owner boundary;
//! actual Task retirement and destructor side effects are covered in QEMU.
use crate::{lifecycle::TeamNode, topology::node};
use kernel_api::model::{identity::TaskId, unit::ExitCause};
use std::sync::Arc;
#[allow(dead_code)]
#[path = "../../src/basis/unit/task/weak.rs"]
mod weak;
pub(crate) use weak::{Origin, TaskWeak};
#[path = "../../src/basis/unit/task/registry.rs"]
mod registry;

pub(crate) struct Team {
    pub node: Arc<TeamNode>,
}
pub(crate) struct Ident {
    pub id: TaskId,
    pub team: Arc<Team>,
}
pub(crate) struct Task {
    pub ident: Ident,
}
pub(crate) struct TaskState;
impl TaskState {
    pub(crate) fn exit(&self) -> Option<(ExitCause, usize)> {
        Some((ExitCause::Reap, 83))
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        registry::remove(self.ident.id);
    }
}
fn publish(id: usize) -> Arc<Task> {
    let task = Arc::new(Task {
        ident: Ident {
            id: TaskId::new(id),
            team: Arc::new(Team {
                node: node(id, None),
            }),
        },
    });
    registry::reserve().unwrap();
    registry::insert(|| (task, ())).unwrap().0
}

#[test]
fn registration_lasts_until_the_last_strong_owner_and_removes_only_that_identity() {
    let first = publish(1);
    let second = publish(2);
    let weak = registry::find(first.ident.id).unwrap();
    let observer = registry::after(0).unwrap();
    assert!(Arc::ptr_eq(&observer, &first));
    drop(first);
    assert!(registry::find(TaskId::new(1)).unwrap().live());
    assert!(weak.upgrade().is_some());
    #[cfg(debug_assertions)]
    assert_eq!(registry::live_ids().0, 0);
    drop(observer);
    assert!(registry::find(TaskId::new(1)).is_none());
    assert!(weak.upgrade().is_none());
    assert!(Arc::ptr_eq(&registry::after(0).unwrap(), &second));
    // Clearing weak registrations must not destroy a live execution; its later
    // last-owner removal is harmless even though shutdown already removed it.
    registry::clear();
    assert!(registry::find(second.ident.id).is_none());
    assert_eq!(Arc::strong_count(&second), 1);
    drop(second);
    assert!(registry::after(0).is_none());
}
