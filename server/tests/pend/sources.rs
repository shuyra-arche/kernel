//! Production weak discovery storage. Payloads are inert host values; these
//! tests establish registry behavior, not Nest or hardware retirement.
use crate::tracking::{CAPACITY, Tracking};
use kernel_api::abi::call::pend::PendFail;
use std::sync::Arc;

#[test]
fn retained_selection_remains_discoverable_without_a_public_slot() {
    let mut sources = Tracking::new();
    let original = Arc::new(17);
    let association = original.clone();
    sources.remember(&original).unwrap();
    drop(original);
    assert!(Arc::ptr_eq(&sources.next().unwrap(), &association));
    // Busy/failure does not acknowledge retirement: merely inspecting the
    // record leaves exactly the same source available on the next pass.
    for _ in 0..8 {
        assert!(Arc::ptr_eq(&sources.next().unwrap(), &association));
    }
    sources.forget(&association);
    assert!(sources.next().is_none());
    assert_eq!(*association, 17);
}

#[test]
fn full_discovery_keeps_old_obligations_and_deduplicates_known_sources() {
    let mut sources = Tracking::new();
    let live: Vec<_> = (0..CAPACITY).map(Arc::new).collect();
    for source in &live {
        sources.remember(source).unwrap();
    }
    let extra = Arc::new(CAPACITY);
    assert_eq!(sources.remember(&extra), Err(PendFail::OutOfMemory));
    for source in &live {
        sources.remember(source).unwrap();
    }
    for source in &live {
        assert!(Arc::ptr_eq(&sources.next().unwrap(), source));
        sources.forget(source);
    }
    assert!(sources.next().is_none());
    sources.remember(&extra).unwrap();
    assert!(Arc::ptr_eq(&sources.next().unwrap(), &extra));
}

#[test]
fn dead_sources_release_capacity_without_retaining_the_payload() {
    let mut sources = Tracking::new();
    let live: Vec<_> = (0..CAPACITY).map(Arc::new).collect();
    let weak = Arc::downgrade(&live[0]);
    for source in &live {
        sources.remember(source).unwrap();
    }
    drop(live);
    assert!(weak.upgrade().is_none());
    let next = Arc::new(99);
    sources.remember(&next).unwrap();
    assert!(Arc::ptr_eq(&sources.next().unwrap(), &next));
    drop(next);
    assert!(sources.next().is_none());
}

#[test]
fn equal_payloads_and_foreign_tracking_do_not_share_cleanup_identity() {
    let mut local = Tracking::new();
    let mut foreign = Tracking::new();
    let first = Arc::new(7);
    let second = Arc::new(7);
    local.remember(&first).unwrap();
    local.remember(&second).unwrap();
    foreign.remember(&first).unwrap();
    local.forget(&second);
    assert!(Arc::ptr_eq(&local.next().unwrap(), &first));
    local.forget(&first);
    assert!(local.next().is_none());
    assert!(Arc::ptr_eq(&foreign.next().unwrap(), &first));
}
