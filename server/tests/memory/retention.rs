use std::sync::{Arc, Barrier};

use super::cell::{Cell, RetainError};
use super::index::PageIndex;
use super::scope::Scope;
use super::slot::{self, Slot};
use super::support::Pool;

#[test]
fn cancelled_preparation_releases_its_obligation_but_not_other_holders() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(pool.page().unwrap()).unwrap());
    let prepared = cell.prepare_hardware().unwrap();
    assert!(!cell.allows_return());
    drop(prepared);
    assert!(cell.allows_return());
    assert_eq!(pool.gives(), 0);
    drop(cell);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn retirement_outlives_the_source_slot_until_explicit_settlement() {
    let pool = Pool::new(0);
    let slots = slot::admit(
        1,
        vec![(
            PageIndex::ZERO,
            Cell::from_internal(pool.page().unwrap()).unwrap(),
        )],
    )
    .unwrap();
    let Slot::Resident(cell) = &slots[0].1 else {
        panic!("resident input")
    };
    let mut retained = cell.prepare_hardware().unwrap();
    let identity = cell.id();
    retained.publish();
    let retired = retained.retire();
    drop(slots);
    assert_eq!(retired.id(), identity);
    assert_eq!(pool.gives(), 0);
    assert!(pool.page().is_err());

    // SAFETY: this host test published no actual hardware access. It exercises
    // the real bookkeeping; it does not establish hardware fence correctness.
    unsafe { retired.settle() };
    assert_eq!(pool.gives(), 1);
    let next = Cell::from_internal(pool.page().unwrap()).unwrap();
    assert_ne!(next.id(), identity);
}

#[test]
fn one_completed_mapping_cannot_discharge_another_mapping() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(pool.page().unwrap()).unwrap());
    let mut a = cell.prepare_hardware().unwrap();
    let mut b = cell.prepare_hardware().unwrap();
    a.publish();
    b.publish();
    let a = a.retire();
    let b = b.retire();
    // SAFETY: no hardware mappings exist in this host test.
    unsafe { a.settle() };
    assert!(!cell.allows_return());
    assert_eq!(pool.gives(), 0);
    // SAFETY: no hardware mappings exist in this host test.
    unsafe { b.settle() };
    assert!(cell.allows_return());
    assert_eq!(pool.gives(), 0);
    drop(cell);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn abandoned_access_remains_quarantined_after_other_accesses_complete() {
    for retire_first in [false, true] {
        let pool = Pool::new(0);
        let cell = Arc::new(Cell::from_internal(pool.page().unwrap()).unwrap());
        let mut lost = cell.prepare_hardware().unwrap();
        let mut completed = cell.prepare_hardware().unwrap();
        let cancelled = cell.prepare_hardware().unwrap();
        lost.publish();
        completed.publish();
        if retire_first {
            drop(lost.retire());
        } else {
            drop(lost);
        }
        drop(cancelled);
        // SAFETY: no hardware mappings exist in this host test.
        unsafe { completed.retire().settle() };
        assert!(!cell.allows_return());
        assert!(matches!(
            cell.prepare_hardware(),
            Err(RetainError::Quarantined)
        ));
        let retained_identity = Arc::downgrade(&cell);
        drop(cell);
        assert!(retained_identity.upgrade().is_some());
        assert_eq!(pool.gives(), 0);
        assert!(pool.page().is_err());
    }
}

#[test]
fn concurrent_settlement_preserves_the_remaining_obligation() {
    let pool = Pool::new(0);
    let cell = Arc::new(Cell::from_internal(pool.page().unwrap()).unwrap());
    let mut remaining = cell.prepare_hardware().unwrap();
    remaining.publish();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let cell = cell.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..64 {
                    let mut retained = cell.prepare_hardware().unwrap();
                    retained.publish();
                    // SAFETY: no hardware mappings exist in this host test.
                    unsafe { retained.retire().settle() };
                    assert!(!cell.allows_return());
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert!(!cell.allows_return());
    drop(cell);
    assert_eq!(pool.gives(), 0);
    // SAFETY: no hardware mappings exist in this host test.
    unsafe { remaining.retire().settle() };
    assert_eq!(pool.gives(), 1);
}

#[test]
fn non_global_updates_cannot_shrink_a_pending_global_scope() {
    let mut scope = Scope::AddressSpace;
    for global in [false, true, false, false] {
        scope = scope.include(global);
    }
    assert_eq!(scope, Scope::Global);
    assert_eq!(Scope::AddressSpace.include(false), Scope::AddressSpace);
}
