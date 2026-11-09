//! Only storage/locking is supplied by this host fixture; links and all topology
//! operations are the production Task chain. RV64 guests cover Task transitions.
use crate::chain::{Chain, Drain, Link};
use std::sync::{Arc, Mutex, MutexGuard};
pub(super) struct Task {
    id: usize,
    pub(super) state: Lock,
}
pub(super) struct Lock(Mutex<State>);
pub(super) struct State(Link);
impl Lock {
    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}
impl State {
    pub(super) fn link(&mut self) -> &mut Link {
        &mut self.0
    }
}
fn task(id: usize) -> Arc<Task> {
    Arc::new(Task {
        id,
        state: Lock(Mutex::new(State(Link::Detached))),
    })
}
fn detached(task: &Task) -> bool {
    task.state.lock().link().detached()
}

#[test]
fn singleton_and_tail_cannot_enter_a_second_queue() {
    let a = task(1);
    let b = task(2);
    let mut first = Chain::new();
    let mut second = Chain::new();
    first.enqueue(a.clone());
    let rejected =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| second.enqueue(a.clone())));
    assert!(rejected.is_err());
    assert!(second.is_empty());
    first.enqueue(b.clone());
    let rejected =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| second.enqueue(b.clone())));
    assert!(rejected.is_err());
    assert!(second.is_empty());
    #[cfg(debug_assertions)]
    assert_eq!(first.len(), 2);
    assert_eq!(first.dequeue().unwrap().id, 1);
    assert_eq!(first.dequeue().unwrap().id, 2);
    assert!(detached(&a) && detached(&b));
    second.enqueue(b.clone());
    assert_eq!(second.dequeue().unwrap().id, 2);
}
#[test]
fn drain_retains_membership_until_removal_and_drop_detaches_the_remainder() {
    let a = task(1);
    let b = task(2);
    let c = task(3);
    let mut chain = Chain::new();
    for task in [&a, &b, &c] {
        chain.enqueue(task.clone());
    }
    let mut drain = chain.take().unwrap();
    assert!(chain.is_empty());
    assert!(!detached(&a) && !detached(&b) && !detached(&c));
    assert_eq!(drain.next().unwrap().id, 1);
    assert!(detached(&a));
    assert!(!detached(&b));
    drop(drain);
    assert!(detached(&b) && detached(&c));
    let mut single = Drain::single(b.clone());
    assert!(!detached(&b));
    assert_eq!(single.next().unwrap().id, 2);
    assert!(detached(&b));
}
#[test]
fn removing_middle_head_and_tail_preserves_the_other_members() {
    let tasks: Vec<_> = (0..5).map(task).collect();
    let mut chain = Chain::new();
    for task in &tasks {
        chain.enqueue(task.clone());
    }
    for id in [2, 0, 4] {
        assert_eq!(chain.remove(&tasks[id]).unwrap().id, id);
        assert!(detached(&tasks[id]));
    }
    assert!(chain.remove(&task(9)).is_none());
    assert_eq!(chain.dequeue().unwrap().id, 1);
    assert_eq!(chain.dequeue().unwrap().id, 3);
    assert!(chain.take().is_none());
}

#[test]
fn moved_drain_and_reused_site_never_share_membership_identity() {
    let a = task(1);
    let b = task(2);
    let c = task(3);
    let mut chain = Chain::new();
    chain.enqueue(a.clone());
    chain.enqueue(b.clone());
    let mut drain = chain.take().unwrap();
    chain.enqueue(c.clone());
    assert!(chain.remove(&a).is_none());
    assert!(chain.remove(&b).is_none());
    assert_eq!(drain.next().unwrap().id, 1);
    assert_eq!(chain.remove(&c).unwrap().id, 3);
    assert_eq!(drain.next().unwrap().id, 2);
    assert!(drain.next().is_none());
}
