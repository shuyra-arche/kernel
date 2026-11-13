#[allow(dead_code)]
#[path = "../../src/basis/sign/wait/key.rs"]
mod key;
#[allow(dead_code)] // Host coverage is admission/retention, not the static shard router.
#[path = "../../src/basis/sign/wait/site.rs"]
mod site;
use crate::{Task, support::Anchor};
use key::WakeKey;
use site::Site;
use std::sync::Arc;

#[test]
fn full_wait_site_rejects_before_linking_and_keeps_all_admitted_waiters() {
    let life = Arc::new(Anchor);
    let mut sites = hashbrown::HashMap::new();
    let key = WakeKey::Sign { id: 1 };
    let site = Site::ensure(&mut sites, key, &Arc::downgrade(&life)).unwrap();
    let tasks: Vec<_> = (0..65).map(|_| Task::new()).collect();
    for task in &tasks[..64] {
        site.admit().unwrap();
        assert!(site.register(task.clone()));
    }
    assert!(site.admit().is_err());
    assert!(tasks[64].state.lock().link().detached());
    assert!(site.has_waiters());
    assert!(site.remove(&tasks[31]).is_some());
    site.admit().unwrap();
    assert!(site.register(tasks[64].clone()));
    let drained: Vec<_> = site.take(true).unwrap().collect();
    assert_eq!(drained.len(), 64);
    assert!(tasks.iter().all(|task| task.state.lock().link().detached()));
}

#[test]
fn shard_exhaustion_does_not_remove_an_existing_site_or_its_notification() {
    let life = Arc::new(Anchor);
    let mut sites = hashbrown::HashMap::new();
    let key = WakeKey::Sign { id: 0 };
    for id in 0..256 {
        Site::ensure(&mut sites, WakeKey::Sign { id }, &Arc::downgrade(&life)).unwrap();
    }
    Site::ensure(&mut sites, key, &Arc::downgrade(&life))
        .unwrap()
        .notify(true);
    assert!(
        Site::ensure(
            &mut sites,
            WakeKey::Sign { id: 256 },
            &Arc::downgrade(&life)
        )
        .is_err()
    );
    assert_eq!(sites.len(), 256);
    assert!(
        Site::ensure(&mut sites, key, &Arc::downgrade(&life))
            .unwrap()
            .take_beacon()
    );
}

#[test]
fn a_retained_notification_is_consumed_once_before_wait_enrollment() {
    let life = Arc::new(Anchor);
    let mut sites = hashbrown::HashMap::new();
    let site = Site::ensure(&mut sites, WakeKey::Sign { id: 1 }, &Arc::downgrade(&life)).unwrap();
    let task = Task::new();
    assert!(site.notify(false).is_none());
    assert!(!site.register(task.clone()));
    assert!(task.state.lock().link().detached());
    assert!(site.register(task.clone()));
    let delivered: Vec<_> = site.notify(true).unwrap().collect();
    assert_eq!(delivered.len(), 1);
    assert!(!site.take_beacon()); // Delivery did not also retain a notification.
    assert!(task.state.lock().link().detached());
}

#[test]
fn dead_source_rejects_enrollment_and_requeue_consumes_its_previous_hint() {
    let life = Arc::new(Anchor);
    let mut sites = hashbrown::HashMap::new();
    let site = Site::ensure(&mut sites, WakeKey::Sign { id: 1 }, &Arc::downgrade(&life)).unwrap();
    let task = Task::new();
    site.notify(true);
    site.requeue(task.clone());
    assert!(!site.take_beacon());
    site.take(true).unwrap().for_each(drop);
    drop(life);
    assert!(!site.register(task.clone()));
    assert!(task.state.lock().link().detached());
}
