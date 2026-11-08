use crate::{
    cell::{Owner, Page},
    index::PageIndex,
    slot::Slot,
    state::FlatSource,
    state::FlatState,
    support::Pool,
    transfer_error::TransferError,
};
use std::sync::Arc;
fn state(slots: Vec<(PageIndex, Slot)>) -> FlatState {
    FlatState::new(FlatSource::Managed(slots), true)
}
#[test]
fn moving_the_unique_slot_preserves_existing_mapping_responsibility() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let identity = page.id();
    let mut memo = page.prepare_hardware(false).unwrap();
    memo.publish();
    let change = page.begin().unwrap();
    let at = PageIndex::ZERO;
    let mut source = state(vec![(at, Slot::Resident(page))]);
    let mut target = state(vec![(at, Slot::Missing)]);
    source.source_ready(at).unwrap();
    assert!(target.target_ready(at).unwrap());
    target.reserve(at).unwrap();
    source.claim_source(at, 7);
    target.claim_target(at, 7, true);
    assert_eq!(source.source_ready(at), Err(TransferError::Busy));
    let page = source.settle_source(at, 7);
    target.settle_target(at, 7, page);
    assert_eq!(source.source_ready(at), Err(TransferError::SourceEmpty));
    assert_eq!(
        target.target_ready(at),
        Err(TransferError::DestinationOccupied)
    );
    assert_eq!(memo.id(), identity);
    drop(change);
    drop(source);
    drop(target);
    assert_eq!(pool.gives(), 0);
    unsafe { memo.retire().settle() };
    assert_eq!(pool.gives(), 1);
}
#[test]
fn failed_movement_restores_fresh_and_missing_destinations_distinctly() {
    for was_missing in [false, true] {
        let pool = Pool::new(0);
        let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
        let at = PageIndex::ZERO;
        let mut source = state(vec![(at, Slot::Resident(page))]);
        let mut target = state(if was_missing {
            vec![(at, Slot::Missing)]
        } else {
            vec![]
        });
        assert_eq!(target.target_ready(at).unwrap(), was_missing);
        target.reserve(at).unwrap();
        source.claim_source(at, 9);
        target.claim_target(at, 9, was_missing);
        source.rollback_source(at, 9);
        target.rollback_target(at, 9);
        assert_eq!(source.source_ready(at), Ok(()));
        assert_eq!(target.target_ready(at), Ok(was_missing));
        drop(source);
        drop(target);
        assert_eq!(pool.gives(), 1);
    }
}
