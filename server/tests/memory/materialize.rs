use crate::{
    cell::{Owner, Page},
    error::FlatError,
    host_lock::SpinLock,
    state::{
        FlatSource, FlatState,
        materialize::{Reservation, Step},
        slot::Slot,
    },
    support::Pool,
    transfer_error::TransferError,
};
use kernel_api::{
    address::{PageCount, PageIndex},
    model::{cell::CellLevel, flat::FlatPhase, identity::TeamId},
};
use std::sync::{Arc, Barrier};

fn source() -> SpinLock<FlatState> {
    SpinLock::new(FlatState::new(FlatSource::Managed(Vec::new()), true))
}
fn reserve(state: &SpinLock<FlatState>, at: usize) -> Result<Reservation<'_>, FlatError> {
    match Reservation::prepare(state, PageIndex::new(at), at + 1, CellLevel::BASE)? {
        Step::Allocate(r) => Ok(r),
        Step::Ready(_) => panic!("expected Fresh"),
    }
}
fn page(pool: &'static Pool) -> Arc<Page> {
    Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap())
}

#[test]
fn concurrent_materializer_residency_and_transfer_see_the_same_busy_position() {
    let state = source();
    let prepared = Barrier::new(2);
    let resume = Barrier::new(2);
    let pool = Pool::new(0);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let r = reserve(&state, 0).unwrap();
            prepared.wait();
            resume.wait();
            r.publish(page(pool), Owner::Kernel.identity()).unwrap();
        });
        prepared.wait();
        // The allocation is paused while holding its reservation, not its lock.
        assert!(matches!(reserve(&state, 0), Err(FlatError::Busy)));
        let guard = state.lock();
        assert!(guard.busy(PageIndex::ZERO, PageCount::new(1).unwrap()));
        assert_eq!(
            guard.source_ready(PageIndex::ZERO),
            Err(TransferError::Busy)
        );
        assert_eq!(
            guard.target_ready(PageIndex::ZERO),
            Err(TransferError::Busy)
        );
        drop(guard);
        resume.wait();
    });
    assert!(matches!(
        Reservation::prepare(&state, PageIndex::ZERO, 1, CellLevel::BASE),
        Ok(Step::Ready(1))
    ));
    drop(state);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn allocation_failure_cancels_only_the_current_slot_and_retry_keeps_the_prefix() {
    let state = source();
    let first = Pool::new(0);
    let cell = page(first);
    let id = cell.id();
    reserve(&state, 0)
        .unwrap()
        .publish(cell, Owner::Kernel.identity())
        .unwrap();
    let failed = reserve(&state, 1).unwrap();
    let exhausted = Pool::new(0);
    let occupied = exhausted.page().unwrap();
    assert!(exhausted.page().is_err());
    drop(failed); // same cancellation path as ? from the real allocator
    drop(occupied);
    let second = Pool::new(0);
    reserve(&state, 1)
        .unwrap()
        .publish(page(second), Owner::Kernel.identity())
        .unwrap();
    let guard = state.lock();
    let FlatSource::Managed(slots) = &guard.source else {
        unreachable!()
    };
    assert_eq!(slots.len(), 2);
    assert!(matches!(&slots[0].1, Slot::Resident(p) if p.id() == id));
    drop(guard);
    drop(state);
    assert_eq!((first.gives(), second.gives()), (1, 1));
}

#[test]
fn metadata_and_arc_allocation_failure_restore_fresh_without_leaking_the_page() {
    let state = source();
    let failed = crate::allocation::fail_after(0, || reserve(&state, 0));
    assert!(matches!(failed, Err(FlatError::OutOfMemory)));
    let r = reserve(&state, 0).unwrap();
    let pool = Pool::new(0);
    let cell = Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap();
    let failed = crate::allocation::fail_after(0, || {
        let cell = Arc::try_new(cell).map_err(|_| FlatError::OutOfMemory)?;
        r.publish(cell, Owner::Kernel.identity())
    });
    assert_eq!(failed, Err(FlatError::OutOfMemory));
    assert_eq!(pool.gives(), 1);
    assert!(reserve(&state, 0).is_ok());
}

#[test]
fn team_exit_adopts_the_inflight_allocation_without_a_second_owner() {
    let state = source();
    let r = reserve(&state, 0).unwrap();
    let owner = Owner::team(TeamId::new(7)).unwrap();
    let pool = Pool::new(0);
    let cell = Arc::new(Page::from_internal(owner.clone(), pool.page().unwrap()).unwrap());
    let id = cell.id();
    owner.close();
    r.publish(cell, owner.identity()).unwrap();
    let guard = state.lock();
    let FlatSource::Managed(slots) = &guard.source else {
        unreachable!()
    };
    assert!(
        matches!(&slots[0].1, Slot::Resident(p) if p.id() == id && p.owner() == Owner::Kernel.identity())
    );
    drop(guard);
    drop(state);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn stale_completion_never_revives_missing_or_cancels_another_reservation() {
    for replacement in [
        Slot::Missing,
        Slot::Allocating {
            reservation: usize::MAX,
        },
    ] {
        let state = source();
        let r = reserve(&state, 0).unwrap();
        // Fault injection at the commit boundary: no legal transfer can replace
        // an Allocating slot, but a stale guard must still fail closed.
        {
            let mut guard = state.lock();
            let FlatSource::Managed(slots) = &mut guard.source else {
                unreachable!()
            };
            slots[0].1 = replacement;
        }
        let pool = Pool::new(0);
        assert_eq!(
            r.publish(page(pool), Owner::Kernel.identity()),
            Err(FlatError::Busy)
        );
        assert_eq!(pool.gives(), 1);
        assert!(matches!(
            reserve(&state, 0),
            Err(FlatError::Missing | FlatError::Busy)
        ));
    }
}

#[test]
fn closure_and_wrong_owner_reject_unpublished_pages_and_release_them() {
    for closing in [false, true] {
        let state = source();
        let r = reserve(&state, 0).unwrap();
        if closing {
            state.lock().phase = FlatPhase::Closing;
        }
        let pool = Pool::new(0);
        let owner = if closing {
            Owner::Kernel
        } else {
            Owner::team(TeamId::new(1)).unwrap()
        };
        assert_eq!(
            r.publish(page(pool), owner.identity()),
            Err(FlatError::Busy)
        );
        assert_eq!(pool.gives(), 1);
        let FlatSource::Managed(slots) = &state.lock().source else {
            unreachable!()
        };
        assert!(slots.is_empty());
    }
}

#[test]
fn cancellation_preserves_unrelated_completed_allocation_and_large_cell_geometry() {
    let state = source();
    let cancelled = reserve(&state, 0).unwrap();
    let pool = Pool::new(0);
    reserve(&state, 1)
        .unwrap()
        .publish(page(pool), Owner::Kernel.identity())
        .unwrap();
    drop(cancelled);
    let large_state = source();
    let large = CellLevel::new(1).unwrap();
    assert!(matches!(
        Reservation::prepare(&large_state, PageIndex::new(1), 512, large),
        Err(FlatError::NotAligned)
    ));
    assert!(matches!(
        Reservation::prepare(&large_state, PageIndex::ZERO, 512, large),
        Ok(Step::Allocate(_))
    ));
    assert!(matches!(
        Reservation::prepare(&state, PageIndex::new(1), 2, CellLevel::BASE),
        Ok(Step::Ready(1))
    ));
}
