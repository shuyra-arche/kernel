#[path = "../../../src/basis/flat/transaction/slots.rs"]
mod slots;
use crate::{
    cell::{Owner, Page},
    index::PageIndex,
    slot::Slot,
    source::FlatSource,
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
    slots::source_ready(&source, at).unwrap();
    assert!(slots::target_ready(&target, at).unwrap());
    slots::reserve(&mut target, at).unwrap();
    slots::claim_source(&mut source, at, 7);
    slots::claim_target(&mut target, at, 7, true);
    assert_eq!(slots::source_ready(&source, at), Err(TransferError::Busy));
    let page = slots::settle_source(&mut source, at, 7);
    slots::settle_target(&mut target, at, 7, page);
    assert_eq!(
        slots::source_ready(&source, at),
        Err(TransferError::SourceEmpty)
    );
    assert_eq!(
        slots::target_ready(&target, at),
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
        assert_eq!(slots::target_ready(&target, at).unwrap(), was_missing);
        slots::reserve(&mut target, at).unwrap();
        slots::claim_source(&mut source, at, 9);
        slots::claim_target(&mut target, at, 9, was_missing);
        slots::rollback_source(&mut source, at, 9);
        slots::rollback_target(&mut target, at, 9);
        assert_eq!(slots::source_ready(&source, at), Ok(()));
        assert_eq!(slots::target_ready(&target, at), Ok(was_missing));
        drop(source);
        drop(target);
        assert_eq!(pool.gives(), 1);
    }
}
