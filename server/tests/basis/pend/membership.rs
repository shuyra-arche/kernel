//! Exercise the production member index without substituting a permission or
//! pretending that discovery proves Nest or hardware retirement.
use crate::members::Members;
use std::sync::Arc;

#[test]
fn membership_scan_has_a_fixed_boundary_and_does_not_reuse_identity() {
    let mut index = Members::new();
    let first = Arc::new(7u8);
    let second = Arc::new(8u8);
    index.prepare().unwrap();
    index.publish(first.clone());
    index.prepare().unwrap();
    index.publish(second.clone());

    let end = index.upper();
    let (old, observed) = index.after(0, end).unwrap();
    assert!(Arc::ptr_eq(&observed, &first));
    index.remove(&first);
    let replacement = Arc::new(7u8);
    index.prepare().unwrap();
    index.publish(replacement.clone());

    assert_eq!(index.after(0, end).unwrap().0, old + 1);
    assert!(index.after(old + 1, end).is_none());
    let (new, observed) = index.after(old + 1, index.upper()).unwrap();
    assert!(new > end);
    assert!(Arc::ptr_eq(&observed, &replacement));
}

#[test]
fn canceled_admission_never_publishes_and_existing_members_survive() {
    let mut index = Members::new();
    let member = Arc::new(1usize);
    index.prepare().unwrap();
    index.prepare().unwrap();
    assert!(!index.is_empty());
    index.cancel();
    index.publish(member.clone());
    assert!(index.after(0, index.upper()).is_some());
    index.remove(&member);
    assert!(index.is_empty());
    index.prepare().unwrap();
    index.cancel();
    assert!(index.is_empty());
}

#[test]
fn scans_follow_publication_order_even_with_several_preparations() {
    let mut index = Members::new();
    let entries: Vec<_> = (0..8).map(Arc::new).collect();
    for _ in &entries {
        index.prepare().unwrap();
    }
    for member in entries.iter().rev() {
        index.publish(member.clone());
    }
    let end = index.upper();
    let mut cursor = 0;
    let mut seen = Vec::new();
    while let Some((sequence, member)) = index.after(cursor, end) {
        assert!(sequence > cursor);
        cursor = sequence;
        seen.push(*member);
    }
    assert_eq!(seen, (0..8).rev().collect::<Vec<_>>());
    assert!(index.after(cursor, end).is_none());
}
