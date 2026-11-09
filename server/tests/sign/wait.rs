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
        site.enqueue(task.clone());
    }
    assert!(site.admit().is_err());
    assert!(tasks[64].state.lock().link().detached());
    assert_eq!(site.chain.len(), 64);
    assert!(site.chain.remove(&tasks[31]).is_some());
    site.admit().unwrap();
    site.enqueue(tasks[64].clone());
    let drained: Vec<_> = site.chain.take().unwrap().collect();
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
        .pend = true;
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
            .pend
    );
}
