//! Execute the production Flat state transitions and Page allocator receipts.
//! Authority, Task dispatch and hardware completion itself are separate gates.
use crate::{
    cell::{Owner, Page},
    index::PageIndex,
    state::{FlatSource, FlatState, residency::Error},
    support::Pool,
};
use kernel_api::model::flat::{FlatVersion, SlotVersion};
use std::sync::{Arc, Barrier, Mutex};

pub(crate) fn fresh() -> FlatState {
    let mut state = FlatState::new(FlatSource::Managed(Vec::new()), true);
    state.explicit = true;
    state
}
pub(crate) fn page(pool: &'static Pool) -> Page {
    Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap()
}
pub(crate) fn resident(state: &FlatState) -> Arc<Page> {
    let FlatSource::Managed(slots) = &state.source else {
        unreachable!()
    };
    let crate::slot::Slot::Resident(page) = &slots[0].1 else {
        unreachable!()
    };
    page.clone()
}

fn maintenance(state: &FlatState) -> Vec<crate::cell::Block> {
    let page = resident(state);
    vec![crate::cell::Block::new(crate::changes::acquire(&page).unwrap()).unwrap()]
}

#[test]
fn closure_is_irreversible_and_does_not_return_any_page_until_all_holdings_settle() {
    use kernel_api::model::flat::FlatPhase;
    let pool = Pool::new(0);
    let second = Pool::new(0);
    let mut state = fresh();
    state
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    state
        .occupy(PageIndex::new(1), SlotVersion::INITIAL, page(second))
        .unwrap();
    let before = state.revision.current();
    let alias = resident(&state);
    let version = state.begin_close(before).unwrap();
    assert_eq!(version, before.next().unwrap());
    assert_eq!(state.phase, FlatPhase::Closing);
    assert_eq!(state.begin_close(before), Err(Error::Stale));
    assert_eq!(state.begin_close(version), Ok(version));
    assert!(matches!(state.finish_close(), Err(Error::Busy)));
    assert_eq!(pool.gives(), 0);
    assert_eq!(
        state.occupy(PageIndex::new(2), SlotVersion::INITIAL, page(Pool::new(0))),
        Err(Error::Busy)
    );
    let weak = Arc::downgrade(&alias);
    drop(alias);
    assert!(matches!(state.finish_close(), Err(Error::Busy)));
    drop(weak);
    let returned = state.finish_close().unwrap();
    assert_eq!(pool.gives() + second.gives(), 0);
    drop(returned);
    assert_eq!(pool.gives() + second.gives(), 2);
    assert_eq!(state.phase, FlatPhase::Closed);
    state.finish_close().unwrap();
    assert_eq!(pool.gives() + second.gives(), 2);
    assert_eq!(state.begin_close(version), Ok(version));
}

#[test]
fn vacate_consumes_only_its_settled_maintenance_reference_without_allocation() {
    let mut state = fresh();
    let pool = Pool::new(0);
    let (stamp, _) = state
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    let mut held = maintenance(&state);
    assert!(resident(&state).changing());
    let (next, returned) =
        crate::allocation::fail_after(0, || state.vacate(PageIndex::ZERO, stamp, Some(&mut held)))
            .unwrap();
    assert!(held.is_empty());
    assert_eq!(next.get(), stamp.get() + 1);
    assert_eq!(pool.gives(), 0);
    drop(returned);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn failed_vacate_keeps_the_same_gate_and_holding_for_weak_and_strong_observers() {
    for weak_only in [false, true] {
        let mut state = fresh();
        let pool = Pool::new(0);
        let (stamp, id) = state
            .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
            .unwrap();
        let observed = resident(&state);
        let weak = Arc::downgrade(&observed);
        let strong = if weak_only {
            drop(observed);
            None
        } else {
            Some(observed)
        };
        let mut held = maintenance(&state);
        let gate = Arc::as_ptr(held[0].change());
        assert_eq!(
            state.vacate(PageIndex::ZERO, stamp, Some(&mut held)).err(),
            Some(Error::Busy)
        );
        assert_eq!(state.revision.at(PageIndex::ZERO), stamp);
        assert_eq!(Arc::as_ptr(held[0].change()), gate);
        let current = weak.upgrade().unwrap();
        assert_eq!(current.id(), id);
        assert!(current.changing());
        assert_eq!(
            current.prepare_hardware(true).err(),
            Some(crate::cell::RetainError::Busy)
        );
        drop(current);
        drop(strong);
        drop(weak);
        drop(
            state
                .vacate(PageIndex::ZERO, stamp, Some(&mut held))
                .unwrap()
                .1,
        );
        assert_eq!(pool.gives(), 1);
    }
}

#[test]
fn overlapping_block_and_inflight_change_prevent_maintenance_consumption() {
    let mut state = fresh();
    let pool = Pool::new(0);
    let (stamp, _) = state
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    let mut held = maintenance(&state);
    let inflight = held[0].change().clone();
    let sibling = crate::cell::Block::new(inflight.clone()).unwrap();
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp, Some(&mut held)).err(),
        Some(Error::Busy)
    );
    drop(sibling);
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp, Some(&mut held)).err(),
        Some(Error::Busy)
    );
    drop(inflight);
    drop(
        state
            .vacate(PageIndex::ZERO, stamp, Some(&mut held))
            .unwrap()
            .1,
    );
    assert_eq!(pool.gives(), 1);
}

#[test]
fn hardware_retirement_and_stale_version_cannot_be_bypassed_by_a_held_block() {
    let mut state = fresh();
    let pool = Pool::new(0);
    let (stamp, _) = state
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    let data = resident(&state);
    let mut receipt = data.prepare_hardware(false).unwrap();
    receipt.publish();
    drop(data);
    let mut held = maintenance(&state);
    assert_eq!(held[0].page().obligations(), 1);
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp, Some(&mut held)).err(),
        Some(Error::Busy)
    );
    // Bookkeeping completion only: no host test installs a physical translation.
    unsafe {
        receipt.retire().settle();
    }
    assert_eq!(
        state
            .vacate(PageIndex::ZERO, SlotVersion::INITIAL, Some(&mut held))
            .err(),
        Some(Error::Stale)
    );
    assert_eq!(held.len(), 1);
    drop(
        state
            .vacate(PageIndex::ZERO, stamp, Some(&mut held))
            .unwrap()
            .1,
    );
    assert_eq!(pool.gives(), 1);
}

#[test]
fn occupy_and_vacate_advance_stamps_and_never_rezero_missing() {
    let mut state = fresh();
    let pool = Pool::new(0);
    let (stamp, id) = state
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    assert_eq!(stamp.get(), 2);
    assert_eq!(state.revision.current().get(), 2);
    assert_eq!(state.revision.at(PageIndex::new(1)), SlotVersion::INITIAL);
    assert_eq!(resident(&state).id(), id);
    assert_eq!(
        state
            .vacate(PageIndex::ZERO, SlotVersion::INITIAL, None)
            .err(),
        Some(Error::Stale)
    );
    let (missing, allocation) = state.vacate(PageIndex::ZERO, stamp, None).unwrap();
    assert_eq!(missing.get(), 3);
    assert_eq!(pool.gives(), 0, "release is outside the source lock");
    drop(allocation);
    assert_eq!(pool.gives(), 1);
    let attempt = Pool::new(0);
    assert_eq!(
        state.occupy(PageIndex::ZERO, missing, page(attempt)),
        Err(Error::Empty)
    );
    assert_eq!(attempt.gives(), 1);
    assert_eq!(state.revision.at(PageIndex::ZERO), missing);
}

#[test]
fn racing_occupy_installs_one_allocation_and_returns_the_loser() {
    let state = Arc::new(Mutex::new(fresh()));
    let barrier = Arc::new(Barrier::new(3));
    let pools = [Pool::new(0), Pool::new(0)];
    let mut workers = Vec::new();
    for pool in pools {
        let state = state.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let prepared = page(pool);
            barrier.wait();
            state
                .lock()
                .unwrap()
                .occupy(PageIndex::ZERO, SlotVersion::INITIAL, prepared)
        }));
    }
    barrier.wait();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().filter(|r| **r == Err(Error::Stale)).count(),
        1
    );
    assert_eq!(pools.iter().map(|p| p.gives()).sum::<usize>(), 1);
    drop(state);
    assert_eq!(pools.iter().map(|p| p.gives()).sum::<usize>(), 2);
}

#[test]
fn vacate_waits_for_software_holders_weak_discovery_and_exact_retirement() {
    let mut state = fresh();
    let pool = Pool::new(0);
    let (stamp, _) = state
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    let observer = resident(&state);
    assert!(observer.allows_return());
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp, None).err(),
        Some(Error::Busy)
    );
    let weak = Arc::downgrade(&observer);
    let mut receipt = observer.prepare_hardware(true).unwrap();
    receipt.publish();
    drop(observer);
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp, None).err(),
        Some(Error::Busy)
    );
    let receipt = receipt.retire();
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp, None).err(),
        Some(Error::Busy)
    );
    // SAFETY: host fixture explicitly models the completion point; it does not
    // claim a real CPU fence or physical-device proof.
    unsafe {
        receipt.settle();
    }
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp, None).err(),
        Some(Error::Busy)
    );
    drop(weak);
    let (_, allocation) = state.vacate(PageIndex::ZERO, stamp, None).unwrap();
    drop(allocation);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn explicit_sources_reject_legacy_transfers_and_unsettled_vacate_keeps_version() {
    let mut state = fresh();
    let pool = Pool::new(0);
    let (stamp, _) = state
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    assert_eq!(
        state.source_ready(PageIndex::ZERO),
        Err(crate::transfer_error::TransferError::NotTransferable)
    );
    let held = resident(&state);
    let change = held.begin().unwrap();
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp, None).err(),
        Some(Error::Busy)
    );
    assert_eq!(state.revision.at(PageIndex::ZERO), stamp);
    drop(change);
    drop(held);
    let (_, allocation) = state.vacate(PageIndex::ZERO, stamp, None).unwrap();
    drop(allocation);
    let mut legacy = FlatState::new(FlatSource::Managed(Vec::new()), true);
    let attempt = Pool::new(0);
    assert_eq!(
        legacy.occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(attempt)),
        Err(Error::Unsupported)
    );
    assert_eq!(legacy.revision.current(), FlatVersion::INITIAL);
    assert_eq!(attempt.gives(), 1);
}

#[test]
fn versioned_transfer_preserves_identity_and_owner_without_moving_unsettled_publications() {
    let mut source = fresh();
    let mut target = fresh();
    let pool = Pool::new(0);
    let (version, id) = source
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    let observed = resident(&source);
    let owner = observed.owner();
    let mut publication = observed.prepare_hardware(false).unwrap();
    publication.publish();
    assert_eq!(
        source.move_to(
            PageIndex::ZERO,
            version,
            &mut target,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            None
        ),
        Err(Error::Busy)
    );
    assert_eq!(source.revision.at(PageIndex::ZERO), version);
    assert_eq!(target.revision.current(), FlatVersion::INITIAL);
    // SAFETY: the host fixture names the completion point, not a physical proof.
    unsafe {
        publication.retire().settle();
    }
    assert_eq!(
        source.move_to(
            PageIndex::ZERO,
            version,
            &mut target,
            PageIndex::ZERO,
            SlotVersion::new(7).unwrap(),
            None
        ),
        Err(Error::Stale)
    );
    let (from, to, moved) = source
        .move_to(
            PageIndex::ZERO,
            version,
            &mut target,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            None,
        )
        .unwrap();
    assert_eq!(moved, id);
    assert_eq!(from.get(), 3);
    assert_eq!(to.get(), 2);
    assert_eq!(resident(&target).owner(), owner);
    assert_eq!(
        observed.id(),
        id,
        "a snapshot is not a second holding or a new authority"
    );
    assert_eq!(
        source.vacate(PageIndex::ZERO, from, None).err(),
        Some(Error::Empty)
    );
    assert_eq!(
        target.vacate(PageIndex::ZERO, to, None).err(),
        Some(Error::Busy)
    );
    drop(observed);
    let (_, allocation) = target.vacate(PageIndex::ZERO, to, None).unwrap();
    drop(allocation);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn same_flat_move_uses_one_revision_and_can_restore_missing_without_zeroing() {
    let mut state = fresh();
    let pool = Pool::new(0);
    let (first, id) = state
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    let (from, to, moved) = state
        .move_within(
            PageIndex::ZERO,
            first,
            PageIndex::new(1),
            SlotVersion::INITIAL,
            None,
        )
        .unwrap();
    assert_eq!(from, to);
    assert_eq!(from.get(), 3);
    assert_eq!(moved, id);
    let (from, to, restored) = state
        .move_within(PageIndex::new(1), to, PageIndex::ZERO, from, None)
        .unwrap();
    assert_eq!(from, to);
    assert_eq!(from.get(), 4);
    assert_eq!(restored, id);
    assert_eq!(
        state.move_within(PageIndex::ZERO, first, PageIndex::new(1), to, None),
        Err(Error::Stale)
    );
    let (_, allocation) = state.vacate(PageIndex::ZERO, to, None).unwrap();
    drop(allocation);
    assert_eq!(pool.gives(), 1);
}
