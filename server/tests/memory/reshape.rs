use crate::{
    cell::Block,
    levels::Pool,
    residency::fresh,
    state::{FlatSource, FlatState, residency::Error},
};
use kernel_api::{
    address::PageIndex,
    model::{
        cell::CellLevel,
        flat::{Reshape, SlotVersion},
    },
};
use std::sync::Arc;
fn fixture() -> (&'static Pool, FlatState, Vec<Block>, Reshape) {
    let pool = Pool::new();
    let mut state = fresh();
    state
        .occupy(
            PageIndex::new(512),
            SlotVersion::INITIAL,
            Arc::try_unwrap(pool.page()).unwrap(),
        )
        .unwrap();
    let FlatSource::Managed(slots) = &state.source else {
        unreachable!()
    };
    let crate::slot::Slot::Resident(page) = &slots[0].1 else {
        unreachable!()
    };
    let held = vec![Block::new(crate::changes::acquire(page).unwrap()).unwrap()];
    (
        pool,
        state,
        held,
        Reshape::new(512, CellLevel::new(1).unwrap()).unwrap(),
    )
}
fn heads(state: &FlatState) -> Vec<(usize, usize)> {
    let FlatSource::Managed(slots) = &state.source else {
        unreachable!()
    };
    slots
        .iter()
        .map(|(at, slot)| {
            let crate::slot::Slot::Resident(page) = slot else {
                panic!("resident")
            };
            (at.get(), page.id().get())
        })
        .collect()
}
#[test]
fn partial_reshape_preserves_origin_and_stamps_all_new_heads_once() {
    let (pool, mut state, mut held, range) = fixture();
    let parent = range.parent();
    let before = heads(&state);
    let old = state.revision.current();
    let (version, stamp) = state.reshape(parent, range, true, old, &mut held).unwrap();
    assert_eq!(version.get(), old.get() + 1);
    assert_eq!(held.len(), 512);
    assert_eq!(state.geometry.head(0, parent), (0, parent));
    for n in 512..1024 {
        assert_eq!(state.geometry.head(n, parent), (n, CellLevel::BASE));
        assert_eq!(state.revision.at(PageIndex::new(n)), stamp);
    }
    assert_eq!(
        state.reshape(parent, range, false, old, &mut held),
        Err(Error::Stale)
    );
    let merged = state
        .reshape(parent, range, false, version, &mut held)
        .unwrap()
        .0;
    assert_eq!(heads(&state).len(), 1);
    assert_ne!(heads(&state), before);
    assert_eq!(held.len(), 1);
    assert!(!state.geometry.covers(513, 1, parent));
    let (_, second) = state
        .reshape(parent, range, true, merged, &mut held)
        .unwrap();
    assert!(second.get() > stamp.get());
    assert_eq!(state.revision.at(PageIndex::new(513)), second);
    drop(held);
    drop(state);
    assert_eq!(pool.used(), 0);
}
#[test]
fn every_preparation_failure_preserves_holding_geometry_version_and_gate() {
    let (pool, mut state, mut held, range) = fixture();
    let before = heads(&state);
    let version = state.revision.current();
    let mut failures = 0;
    loop {
        let result = crate::allocation::fail_after(failures, || {
            state.reshape(range.parent(), range, true, version, &mut held)
        });
        if result.is_ok() {
            break;
        }
        assert_eq!(result, Err(Error::OutOfMemory));
        assert_eq!(heads(&state), before);
        assert_eq!(state.revision.current(), version);
        assert_eq!(held.len(), 1);
        assert!(held[0].page().changing());
        assert_eq!(
            state.geometry.head(513, range.parent()),
            (512, range.parent())
        );
        failures += 1;
        assert!(failures < 1100);
    }
    assert!(failures >= 1024);
    drop(held);
    drop(state);
    assert_eq!(pool.used(), 0);
}
#[test]
fn aliases_overlapping_gates_and_pending_hardware_refuse_before_conversion() {
    for mode in 0..3 {
        let (pool, mut state, mut held, range) = fixture();
        let version = state.revision.current();
        let before = heads(&state);
        let alias = (mode == 0).then(|| Arc::downgrade(held[0].page()));
        let overlap = (mode == 1).then(|| Block::new(held[0].change().clone()).unwrap());
        let hardware = (mode == 2).then(|| held[0].permit().unwrap().hardware(true).unwrap());
        assert_eq!(
            state.reshape(range.parent(), range, true, version, &mut held),
            Err(Error::Busy)
        );
        assert_eq!(heads(&state), before);
        assert_eq!(state.revision.current(), version);
        drop(alias);
        drop(overlap);
        drop(hardware);
        state
            .reshape(range.parent(), range, true, version, &mut held)
            .unwrap();
        drop(held);
        drop(state);
        assert_eq!(pool.used(), 0);
    }
}
