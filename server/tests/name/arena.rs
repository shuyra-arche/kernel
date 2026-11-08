include!("../../src/basis/name/arena.rs");

#[test]
fn wrong_table_stale_and_unpublished_keys_never_resolve() {
    let mut a = Arena::<String, ()>::new();
    let mut b = Arena::<String, ()>::new();
    let first = a.reserve().unwrap();
    let foreign = b.reserve().unwrap();
    assert!(a.get(first).is_none());
    assert!(a.get(foreign).is_none());
    assert!(a.publish(foreign, "foreign".into()).is_err());
    assert!(!a.cancel(foreign));
    assert!(a.remove(first).is_none());
    a.publish(first, "first".into()).unwrap();
    assert!(!a.cancel(first));
    assert!(a.publish(first, "overwrite".into()).is_err());
    assert_eq!(a.remove(first).as_deref(), Some("first"));
    let second = a.reserve().unwrap();
    assert_eq!(first.slot, second.slot);
    assert!(first != second);
    a.publish(second, "second".into()).unwrap();
    assert!(a.get(first).is_none());
    assert!(a.remove(first).is_none());
    assert!(!a.cancel(first));
    assert_eq!(a.get(second).map(String::as_str), Some("second"));
    assert!(b.cancel(foreign));
    let fresh = Arena::<String, ()>::new();
    assert!(fresh.get(second).is_none());
    assert_eq!(a.into_values().collect::<Vec<_>>(), ["second"]);
}

#[test]
fn cancellation_invalidates_reservations_and_reuses_storage() {
    let mut arena = Arena::<usize, ()>::new();
    let old = arena.reserve().unwrap();
    assert!(arena.cancel(old));
    let new = arena.reserve().unwrap();
    assert_eq!(old.slot, new.slot);
    assert!(!arena.cancel(old));
    assert_eq!(arena.publish(old, 9), Err(9));
    assert_eq!(arena.len(), 0);
    assert_eq!(arena.iter().count(), 0);
    arena.publish(new, 7).unwrap();
    for value in 0..1000 {
        let key = arena.reserve().unwrap();
        arena.publish(key, value).unwrap();
        assert_eq!(arena.remove(key), Some(value));
    }
    assert_eq!(arena.slots.len(), 2);
    assert_eq!(arena.iter().copied().collect::<Vec<_>>(), [7]);
}

#[test]
fn exhausted_slots_are_retired_on_cancel_and_remove() {
    for publish in [false, true] {
        let mut arena = Arena::<usize, ()>::new();
        let mut last = arena.reserve().unwrap();
        // Inject the actual counter limit without requiring 2^64 operations.
        last.generation = super::generation::last();
        arena.slots[last.slot].generation = last.generation;
        if publish {
            arena.publish(last, 1).unwrap();
            assert_eq!(arena.remove(last), Some(1));
        } else {
            assert!(arena.cancel(last));
        }
        let new = arena.reserve().unwrap();
        assert_ne!(new.slot, last.slot);
        assert!(arena.get(last).is_none());
        assert!(!arena.cancel(last));
        assert!(arena.publish(last, 2).is_err());
        assert_eq!(arena.len(), 0);
    }
}

#[test]
fn allocation_failure_does_not_publish_or_lose_reusable_slots() {
    let mut arena = Arena::<usize, ()>::new();
    assert!(super::allocation::fail_after(0, || arena.reserve()).is_none());
    assert_eq!(arena.len(), 0);
    let key = arena.reserve().unwrap();
    arena.publish(key, 1).unwrap();
    assert_eq!(arena.remove(key), Some(1));
    let reused = super::allocation::fail_after(0, || arena.reserve()).unwrap();
    assert_eq!(key.slot, reused.slot);
    assert!(key != reused);
}
