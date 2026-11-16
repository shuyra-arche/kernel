//! Exercise the production watch's ownership, not CPU fencing or Task admission.
//! The scalar state fixture supplies a deterministic pause during observation.
use std::{
    cell::Cell,
    marker::PhantomData,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
};
#[path = "../../../../src/basis/room/shutdown/root.rs"]
mod mechanism;

mod order {
    pub(super) struct TaskState;
}
struct LegacyLock<C, T> {
    value: Mutex<T>,
    class: PhantomData<C>,
}
impl<C, T> LegacyLock<C, T> {
    fn new(value: T) -> Self {
        Self {
            value: Mutex::new(value),
            class: PhantomData,
        }
    }
    fn lock(&self) -> MutexGuard<'_, T> {
        self.value.lock().unwrap()
    }
}
struct TaskState {
    dead: bool,
    pause: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}
impl TaskState {
    fn is_dead(&self) -> bool {
        if let Some((entered, release)) = &self.pause {
            entered.send(()).unwrap();
            release.recv().unwrap();
        }
        self.dead
    }
}
thread_local! { static COMMITTED: Cell<bool> = const { Cell::new(false) }; }
struct Memory(Arc<AtomicUsize>);
impl Drop for Memory {
    fn drop(&mut self) {
        assert!(
            !COMMITTED.get(),
            "Task resource destruction under an observer's COMMIT"
        );
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct Task {
    state: Arc<LegacyLock<order::TaskState, TaskState>>,
    _memory: Memory,
}

#[test]
fn shutdown_observation_cannot_become_the_last_owner_of_task_resources() {
    let root = Arc::new(mechanism::Root::new());
    assert!(!root.active());
    let (entered, inside) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let state = Arc::new(LegacyLock::new(TaskState {
        dead: false,
        pause: Some((entered, wait)),
    }));
    let released = Arc::new(AtomicUsize::new(0));
    let task = Arc::new(Task {
        state: state.clone(),
        _memory: Memory(released.clone()),
    });
    root.watch(&task.state).unwrap();
    assert!(root.watch(&task.state).is_err());
    let observed = root.clone();
    let thread = std::thread::spawn(move || {
        COMMITTED.set(true);
        let active = observed.active();
        COMMITTED.set(false);
        active
    });
    inside.recv().unwrap();
    // The observer is paused while holding the state read. Releasing the last
    // real Task owner must still destroy its resource outside that observer.
    drop(task);
    assert_eq!(released.load(Ordering::SeqCst), 1);
    release.send(()).unwrap();
    assert!(!thread.join().unwrap());
    {
        let mut state = state.lock();
        state.pause = None;
        state.dead = true;
    }
    assert!(root.active());
    let weak = Arc::downgrade(&state);
    drop(state);
    assert!(
        weak.upgrade().is_none(),
        "watch retained authoritative state storage"
    );
    assert!(root.active()); // Disappeared state retains the stopped observation.
}
