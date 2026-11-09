use super::{Task, controls::Controls};
use crate::basis::nest::{Nest, NestError};
use kernel_api::model::identity::TaskId;
use std::sync::{Arc, atomic::Ordering};

#[test]
fn each_turn_advances_one_stage_without_replaying_completed_effects() {
    let task = Task::new();
    assert_eq!(task.retire(), Err(NestError::Busy)); // no heirs
    task.pend_busy.store(true, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    task.pend_busy.store(false, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(task.retire(), Err(NestError::Busy)); // no controls
    task.ident.busy.store(true, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    task.ident.busy.store(false, Ordering::Release);
    for _ in 0..8 {
        if task.retire() == Ok(()) {
            break;
        }
    }
    assert_eq!(task.retire(), Ok(()));
    assert_eq!(task.retire(), Ok(()));
    assert_eq!(&*task.trace.lock(), &["pends", "pends", "memory", "memory"]);
}

#[test]
fn removing_an_earlier_child_does_not_skip_the_next_child() {
    let task = Task::new();
    let first = task.child(3);
    let next = task.child(8);
    first.ready.store(true, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    task.heir.lock().remove(0);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(&*task.trace.lock(), &["team", "team"]);
    next.ready.store(true, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(&*task.trace.lock(), &["team", "team", "team", "pends"]);
}

#[test]
fn concurrent_retirement_rejects_before_running_any_effect() {
    let task = Task::new();
    let owner = task.retirement.lock();
    let (sent, received) = std::sync::mpsc::channel();
    let other = task.clone();
    let worker = std::thread::spawn(move || sent.send(other.retire()).unwrap());
    // Rejection must arrive before the owner releases. A watchdog makes a
    // regression to blocking acquisition fail without hanging the test suite.
    let rejected = received.recv_timeout(std::time::Duration::from_secs(5));
    drop(owner);
    worker.join().unwrap();
    assert_eq!(rejected.unwrap(), Err(NestError::Busy));
    assert!(task.trace.lock().is_empty());
}

#[test]
fn busy_mapping_does_not_starve_other_nests_or_lose_its_record() {
    let controls = Arc::new(Controls::new());
    let actor = TaskId::new(1);
    let busy = Nest::new(&controls, actor, &[3]);
    let ready = Nest::new(&controls, actor, &[4, 5]);
    busy.block(true);
    assert_eq!(controls.retire(actor), Err(NestError::Busy));
    assert_eq!(controls.retire(actor), Err(NestError::Busy));
    assert_eq!(controls.retire(actor), Err(NestError::Busy));
    assert_eq!(&*ready.attempts.lock(), &[4, 5]);
    assert!(busy.mappings.registered(3).is_some());
    busy.block(false);
    assert_eq!(controls.retire(actor), Ok(()));
    assert_eq!(&*busy.attempts.lock(), &[3, 3]);
}

#[test]
fn concurrent_publication_cannot_be_mistaken_for_mapping_settlement() {
    let controls = Arc::new(Controls::new());
    let actor = TaskId::new(1);
    let nest = Nest::new(&controls, actor, &[3]);
    nest.republish(true);
    assert_eq!(controls.retire(actor), Err(NestError::Busy));
    assert!(nest.mappings.registered(3).is_some());
    nest.republish(false);
    assert_eq!(controls.retire(actor), Ok(()));
    assert_eq!(&*nest.attempts.lock(), &[3, 3]);
}

#[test]
fn missing_nest_drops_only_its_stale_control_record() {
    let controls = Arc::new(Controls::new());
    let actor = TaskId::new(1);
    let stale = Nest::new(&controls, actor, &[3]);
    let live = Nest::new(&controls, actor, &[4]);
    live.block(true);
    drop(stale);
    assert_eq!(controls.retire(actor), Err(NestError::Busy));
    assert!(live.attempts.lock().is_empty());
    assert_eq!(controls.retire(actor), Err(NestError::Busy));
    live.block(false);
    assert_eq!(controls.retire(actor), Ok(()));
}

#[test]
fn pending_controls_hold_the_task_before_its_memory_is_retired() {
    let task = Task::new();
    let nest = Nest::new(&task.controls, task.ident.id, &[3]);
    nest.block(true);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(&*task.trace.lock(), &["pends"]);
    nest.block(false);
    assert_eq!(task.retire(), Err(NestError::Busy));
    for _ in 0..8 {
        if task.retire() == Ok(()) {
            break;
        }
    }
    assert_eq!(task.retire(), Ok(()));
    assert_eq!(&*task.trace.lock(), &["pends", "memory"]);
}
