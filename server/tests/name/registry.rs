include!("../../src/basis/name/registry.rs");

#[test]
fn index_and_reservations_commit_or_rollback_together() {
    let mut registry = Registry::<usize, usize>::default();
    for fail in [0, 1] {
        let mut failed = Registry::<usize, usize>::default();
        assert!(super::allocation::fail_after(fail, || failed.reserve(4)).is_none());
        assert!(failed.index.is_empty());
        assert_eq!(failed.len(), 0);
        failed.insert(4, 40).unwrap();
        assert_eq!(failed.get(4), Some(&40));
    }
    let key = registry.reserve(1).unwrap();
    assert!(registry.get(1).is_none());
    assert!(registry.reserve(1).is_none());
    assert!(registry.remove(1).is_none());
    assert_eq!(registry.publish(2, key, 10), Err(10));
    assert!(!registry.cancel(2, key));
    assert!(registry.cancel(1, key));
    registry.insert(2, 20).unwrap();
    assert!(!registry.cancel(1, key));
    assert_eq!(registry.get(1), None);
    assert_eq!(registry.get(2), Some(&20));
    assert_eq!(registry.iter().copied().collect::<Vec<_>>(), [20]);
    assert_eq!(registry.insert(2, 30), Err(30));
    assert_eq!(registry.into_values().collect::<Vec<_>>(), [20]);
}

#[test]
fn retained_reference_is_the_same_record_after_slot_reuse() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let mut registry = Registry::<usize, Arc<AtomicBool>>::default();
    let record = Arc::new(AtomicBool::new(true));
    registry.insert(1, record.clone()).unwrap();
    let retained = registry.get(1).unwrap().clone();
    assert!(Arc::ptr_eq(&retained, &record));
    // The authority owner invalidates before removal; the arena does not revoke.
    record.store(false, Ordering::Release);
    registry.remove(1).unwrap();
    registry.insert(2, Arc::new(AtomicBool::new(true))).unwrap();
    assert!(!retained.load(Ordering::Acquire));
    assert!(registry.get(1).is_none());
    assert!(registry.get(2).unwrap().load(Ordering::Acquire));
}
