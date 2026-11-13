use crate::cell::Owner;
use std::sync::Arc;
use std::sync::Barrier;

use super::cell::Page;
use super::cell::RetainError;
use super::index::PageIndex;
use super::scope::Scope;
use super::slot;
use super::slot::Slot;
use super::support::Pool;

#[test]
fn cancelled_preparation_releases_its_obligation_but_not_other_holders() {
    let pool = Pool::new(0);
    let cell = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let prepared = cell.prepare_hardware(false).unwrap();
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
            Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap(),
        )],
    )
    .unwrap();
    let Slot::Resident(cell) = &slots[0].1 else {
        panic!("resident input")
    };
    let mut retained = cell.prepare_hardware(false).unwrap();
    let identity = cell.id();
    retained.publish();
    let retired = retained.retire();
    let telemetry = crate::cell::statistics::snapshot();
    assert!(telemetry.cpu >= 4096 && telemetry.retiring >= 4096);
    drop(slots);
    assert_eq!(retired.id(), identity);
    assert_eq!(pool.gives(), 0);
    assert!(pool.page().is_err());

    // SAFETY: this host test published no actual hardware access. It exercises
    // the real bookkeeping; it does not establish hardware fence correctness.
    unsafe { retired.settle() };
    assert_eq!(pool.gives(), 1);
    let next = Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap();
    assert_ne!(next.id(), identity);
}

#[test]
fn one_completed_mapping_cannot_discharge_another_mapping() {
    let pool = Pool::new(0);
    let cell = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let mut a = cell.prepare_hardware(false).unwrap();
    let mut b = cell.prepare_hardware(false).unwrap();
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
        let cell = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
        let mut lost = cell.prepare_hardware(false).unwrap();
        let mut completed = cell.prepare_hardware(false).unwrap();
        let cancelled = cell.prepare_hardware(false).unwrap();
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
            cell.prepare_hardware(false),
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
    let cell = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let mut remaining = cell.prepare_hardware(false).unwrap();
    remaining.publish();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let cell = cell.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..64 {
                    let mut retained = cell.prepare_hardware(false).unwrap();
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

#[test]
fn shared_readers_block_writers_across_cpu_and_device_until_settled() {
    let pool = Pool::new(0);
    let cell = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let mut cpu = cell.prepare_hardware(false).unwrap();
    let mut device = cell.prepare_external(false).unwrap();
    cpu.publish();
    device.publish();
    assert!(matches!(
        cell.prepare_hardware(true),
        Err(RetainError::Borrowed)
    ));
    assert!(matches!(
        cell.prepare_external(true),
        Err(RetainError::Borrowed)
    ));
    let retiring = cpu.retire();
    // Retirement is an intention, not a completion witness.
    assert!(matches!(
        cell.prepare_hardware(true),
        Err(RetainError::Borrowed)
    ));
    unsafe { retiring.settle() };
    assert!(matches!(
        cell.prepare_hardware(true),
        Err(RetainError::Borrowed)
    ));
    unsafe { device.retire().settle() };
    let writer = cell.prepare_hardware(true).unwrap();
    assert!(matches!(
        cell.prepare_external(false),
        Err(RetainError::Borrowed)
    ));
    let reader = cell.prepare_hardware(false).unwrap();
    drop(reader);
    drop(writer); // Unpublished preparation may be cancelled.
    assert!(cell.allows_return());
}

#[test]
fn competing_writers_share_cpu_or_exclusively_reserve_external_access() {
    let pool = Pool::new(0);
    let cell = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let start = Arc::new(Barrier::new(8));
    let finish = Arc::new(Barrier::new(8));
    let winners = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8)
            .map(|index| {
                let cell = cell.clone();
                let start = start.clone();
                let finish = finish.clone();
                scope.spawn(move || {
                    start.wait();
                    let result = if index % 2 == 0 {
                        cell.prepare_external(true)
                    } else {
                        cell.prepare_hardware(true)
                    };
                    finish.wait(); // Keep the winning reservation through every attempt.
                    match result {
                        Ok(_) => 1,
                        Err(RetainError::Borrowed) => 0,
                        other => panic!("unexpected admission: {other:?}"),
                    }
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .sum::<usize>()
    });
    assert!(matches!(winners, 1 | 4));
    assert!(cell.allows_return());
}

#[test]
fn publication_cannot_complete_or_retire_an_unpublished_preparation() {
    use super::memo::Publication;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let mut prepared = Publication::prepare(&page, false, None).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| prepared.complete())).is_err());
    assert!(matches!(prepared.phase(), crate::memo::Phase::Prepared));
    drop(prepared);
    assert!(page.allows_return());
    let prepared = Publication::prepare(&page, false, None).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| prepared.retire())).is_err());
    assert!(page.allows_return());
    let mut published = Publication::prepare(&page, false, None).unwrap();
    published.publish();
    assert!(catch_unwind(AssertUnwindSafe(|| published.publish())).is_err());
    published.complete();
    // No hardware mapping exists in this host bookkeeping test.
    unsafe { published.retire().settle() };
    assert!(page.allows_return());
}
