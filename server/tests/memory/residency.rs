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

fn fresh() -> FlatState {
    let mut state = FlatState::new(FlatSource::Managed(Vec::new()), true);
    state.explicit = true;
    state
}
fn page(pool: &'static Pool) -> Page {
    Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap()
}
fn resident(state: &FlatState) -> Arc<Page> {
    let FlatSource::Managed(slots) = &state.source else {
        unreachable!()
    };
    let crate::slot::Slot::Resident(page) = &slots[0].1 else {
        unreachable!()
    };
    page.clone()
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
        state.vacate(PageIndex::ZERO, SlotVersion::INITIAL).err(),
        Some(Error::Stale)
    );
    let (missing, allocation) = state.vacate(PageIndex::ZERO, stamp).unwrap();
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
        state.vacate(PageIndex::ZERO, stamp).err(),
        Some(Error::Busy)
    );
    let weak = Arc::downgrade(&observer);
    let mut receipt = observer.prepare_hardware(true).unwrap();
    receipt.publish();
    drop(observer);
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp).err(),
        Some(Error::Busy)
    );
    let receipt = receipt.retire();
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp).err(),
        Some(Error::Busy)
    );
    // SAFETY: host fixture explicitly models the completion point; it does not
    // claim a real CPU fence or physical-device proof.
    unsafe {
        receipt.settle();
    }
    assert_eq!(
        state.vacate(PageIndex::ZERO, stamp).err(),
        Some(Error::Busy)
    );
    drop(weak);
    let (_, allocation) = state.vacate(PageIndex::ZERO, stamp).unwrap();
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
        state.vacate(PageIndex::ZERO, stamp).err(),
        Some(Error::Busy)
    );
    assert_eq!(state.revision.at(PageIndex::ZERO), stamp);
    drop(change);
    drop(held);
    let (_, allocation) = state.vacate(PageIndex::ZERO, stamp).unwrap();
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
