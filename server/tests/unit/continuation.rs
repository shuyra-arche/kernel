use super::{Task, retirement::Retirement};
use crate::basis::nest::NestError;
use kernel_api::model::identity::TaskId;
use std::sync::{Arc, atomic::Ordering};

#[test]
fn each_turn_advances_one_stage_without_replaying_completed_effects() {
    let task = Task::new();
    assert!(matches!(*task.retirement.lock(), Retirement::Heirs { .. }));
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert!(matches!(*task.retirement.lock(), Retirement::Pends));
    assert!(task.trace.lock().is_empty());

    task.pend_busy.store(true, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert!(matches!(*task.retirement.lock(), Retirement::Pends));
    task.pend_busy.store(false, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert!(matches!(*task.retirement.lock(), Retirement::Memory));
    assert!(!task.ident.retired.load(Ordering::Acquire));
    assert_eq!(&*task.trace.lock(), &["pends", "pends"]);

    task.ident.busy.store(true, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert!(matches!(*task.retirement.lock(), Retirement::Memory));
    task.ident.busy.store(false, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert!(matches!(*task.retirement.lock(), Retirement::Receipts(_)));
    assert!(task.ident.retired.load(Ordering::Acquire));
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert!(matches!(*task.retirement.lock(), Retirement::Receipts(_)));
    assert_eq!(task.retire(), Ok(()));
    assert!(matches!(*task.retirement.lock(), Retirement::Settled));
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
    assert!(matches!(
        *task.retirement.lock(),
        Retirement::Heirs { after: 3 }
    ));
    assert!(!task.ident.retired.load(Ordering::Acquire));
    assert_eq!(&*task.trace.lock(), &["team", "team"]);
    next.ready.store(true, Ordering::Release);
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert_eq!(task.retire(), Err(NestError::Busy));
    assert!(matches!(*task.retirement.lock(), Retirement::Memory));
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
fn cancelled_admission_retains_resources_until_preparation_releases_its_lease() {
    let task = Task::new();
    task.preparing.store(true, Ordering::Release);
    let (sent, received) = std::sync::mpsc::channel();
    let other = task.clone();
    let worker = std::thread::spawn(move || {
        for _ in 0..16 {
            assert_eq!(other.retire(), Err(NestError::Busy));
        }
        sent.send(()).unwrap();
    });
    // A stopped Task's Arc does not retain its context page: its preparation
    // lease must reject retirement before any Pend or memory effect occurs.
    let retained = received.recv_timeout(std::time::Duration::from_secs(5));
    task.preparing.store(false, Ordering::Release);
    worker.join().unwrap();
    retained.unwrap();
    assert!(task.trace.lock().is_empty());
    for _ in 0..8 {
        if task.retire() == Ok(()) {
            break;
        }
    }
    assert_eq!(task.retire(), Ok(()));
    assert_eq!(&*task.trace.lock(), &["pends", "memory"]);
}

// Housekeeping retains one strong owner on Busy and publishes reclamation only
// after the production continuation succeeds. Queueing and hardware stay external.
fn advance_husk(husk: &mut Option<Arc<Task>>) -> Result<(), NestError> {
    let task = husk.as_ref().unwrap();
    task.retire()?;
    if let Some(member) = task.ident.node.member(task.ident.id) {
        member.reclaim();
    }
    *husk = None;
    Ok(())
}

#[test]
fn pending_pend_and_original_source_keep_husk_memory_and_receipts_until_proof() {
    let task = Task::new();
    let node = task.ident.node.clone();
    let owner = task.ident.id;
    let member = crate::topology::member(&node, owner.get(), &[9]);
    let result = crate::topology::member(&node, 2, &[owner.get(), 7]);
    let weak = Arc::downgrade(&task);
    let trace = task.trace.clone();
    task.pend_busy.store(true, Ordering::Release);
    task.source_busy.store(true, Ordering::Release);
    let mut husk = Some(task);

    assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
    for _ in 0..4 {
        assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
        let task = husk.as_ref().unwrap();
        assert!(matches!(*task.retirement.lock(), Retirement::Pends));
        assert!(!task.ident.retired.load(Ordering::Acquire));
        assert!(!member.reclaimed.load(Ordering::Acquire));
        assert!(result.pending(owner));
        assert_eq!(weak.strong_count(), 1);
    }

    // Empty Pend entries, terminal state and notifications do not acknowledge
    // outstanding original-source publications in a foreign address space.
    husk.as_ref()
        .unwrap()
        .pend_busy
        .store(false, Ordering::Release);
    assert!(node.all_reaped());
    node.notify();
    for _ in 0..4 {
        assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
        let task = husk.as_ref().unwrap();
        assert!(matches!(*task.retirement.lock(), Retirement::Pends));
        assert!(!task.ident.retired.load(Ordering::Acquire));
        assert!(!member.reclaimed.load(Ordering::Acquire));
        assert!(result.pending(owner));
        assert_eq!(weak.strong_count(), 1);
    }
    assert_eq!(&*trace.lock(), &["pends"; 8]);

    // This is the effect boundary's explicit withdrawal-completion proof, not
    // evidence of CPU or device completion on real hardware.
    husk.as_ref()
        .unwrap()
        .source_busy
        .store(false, Ordering::Release);
    assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
    let task = husk.as_ref().unwrap();
    assert!(matches!(*task.retirement.lock(), Retirement::Memory));
    assert!(!task.ident.retired.load(Ordering::Acquire));
    assert!(result.pending(owner));

    task.ident.busy.store(true, Ordering::Release);
    assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
    let task = husk.as_ref().unwrap();
    assert!(matches!(*task.retirement.lock(), Retirement::Memory));
    assert!(!member.reclaimed.load(Ordering::Acquire));
    assert!(result.pending(owner));
    task.ident.busy.store(false, Ordering::Release);
    assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
    let task = husk.as_ref().unwrap();
    assert!(matches!(*task.retirement.lock(), Retirement::Receipts(_)));
    assert!(task.ident.retired.load(Ordering::Acquire));
    assert!(result.pending(owner));
    assert!(!member.reclaimed.load(Ordering::Acquire));

    assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
    assert!(result.pending(owner));
    assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
    assert!(!result.pending(owner));
    assert!(result.pending(TaskId::new(7)));
    assert!(!member.reclaimed.load(Ordering::Acquire));
    assert_eq!(advance_husk(&mut husk), Err(NestError::Busy));
    assert_eq!(advance_husk(&mut husk), Ok(()));
    assert!(husk.is_none());
    assert!(weak.upgrade().is_none());
    assert!(member.reclaimed.load(Ordering::Acquire));
    assert!(member.pending(TaskId::new(9)));
    assert_eq!(*result.result.lock(), Some(83));
    let mut expected = vec!["pends"; 9];
    expected.extend(["memory", "memory"]);
    assert_eq!(&*trace.lock(), &expected);
}
