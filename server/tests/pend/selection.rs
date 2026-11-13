//! Exercise the production live Pend, not a copied permission model. These
//! checks do not execute trap dispatch, allocate Pages or prove a transfer.
use crate::authority::Pend;
use kernel_api::model::{
    cell::MappingId,
    modifiable::Modifiable::{No, Yes},
    pend::{
        Distributable, FlatRights, Limits, NestRights, NestScope, Operations, PendRange, PendScope,
        ShareRights,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn pages(offset: usize, count: usize) -> PendScope {
    PendScope::Pages(PendRange::Pages { offset, count })
}
fn mapping(id: usize, offset: usize, count: usize) -> PendScope {
    PendScope::Nest(NestScope::Mapping {
        id: MappingId::from_bytes(&(id as u64).to_le_bytes()).unwrap(),
        range: PendRange::Pages { offset, count },
    })
}
fn flat() -> Arc<Pend> {
    Pend::try_new_scope(
        Yes,
        pages(10, 8),
        Operations::Flat(FlatRights::all()),
        Distributable::Yes,
    )
    .unwrap()
}
const INSPECT: Operations = Operations::Flat(FlatRights::INSPECT);
const TRANSFER: Operations = Operations::Flat(FlatRights::TRANSFER);

#[test]
fn fixed_flat_pages_survive_narrow_without_rebasing() {
    let grant = flat();
    for page in [10, 12, 17] {
        assert!(grant.selects(INSPECT, pages(page, 1)));
    }
    grant
        .prepare_limit(Limits {
            scope: pages(12, 2),
            ..grant.limits()
        })
        .unwrap()
        .commit();
    assert!(grant.selects(INSPECT, pages(12, 1)));
    assert!(grant.selects(INSPECT, pages(13, 1)));
    for page in [0, 1, 10, 11, 14, 17] {
        assert!(!grant.selects(INSPECT, pages(page, 1)));
    }
    assert!(!grant.selects(INSPECT, PendScope::Pages(PendRange::Inherit)));
}

#[test]
fn mapping_selection_requires_original_mapping_identity_and_complete_window() {
    let rights = Operations::Nest(NestRights::PUBLISH);
    let grant = Pend::try_new_scope(Yes, mapping(41, 10, 8), rights, Distributable::Yes).unwrap();
    assert!(grant.selects(rights, mapping(41, 12, 2)));
    for scope in [
        mapping(42, 12, 2),
        pages(12, 2),
        PendScope::Nest(NestScope::Whole),
        mapping(41, 17, 2),
    ] {
        assert!(!grant.selects(rights, scope));
    }
    grant
        .prepare_limit(Limits {
            scope: mapping(41, 12, 2),
            ..grant.limits()
        })
        .unwrap()
        .commit();
    assert!(grant.selects(rights, mapping(41, 12, 2)));
    assert!(!grant.selects(rights, mapping(41, 0, 2)));
    let whole = Pend::try_new_scope(
        Yes,
        PendScope::Nest(NestScope::Whole),
        Operations::Nest(NestRights::whole()),
        Distributable::Yes,
    )
    .unwrap();
    assert!(!whole.selects(Operations::Nest(NestRights::INSPECT), mapping(41, 12, 2)));
}

#[test]
fn endpoint_permissions_are_rechecked_after_preparation() {
    let source = flat();
    let target = flat();
    let selection = pages(12, 1);
    assert!(source.selects(TRANSFER, selection) && target.selects(TRANSFER, selection));
    // Keep both ranges and ASSOCIATE live, but remove movement from one endpoint.
    target
        .prepare_limit(Limits {
            operations: Operations::Flat(FlatRights::ASSOCIATE | FlatRights::INSPECT),
            ..target.limits()
        })
        .unwrap()
        .commit();
    assert!(target.selects(Operations::Flat(FlatRights::ASSOCIATE), selection));
    assert!(source.selects(TRANSFER, selection));
    assert!(!target.selects(TRANSFER, selection));
    source.invalidate();
    assert!(!source.selects(TRANSFER, selection));
}

#[test]
fn selection_observes_ancestor_revocation_and_holder_death() {
    let parent = flat();
    let child = flat();
    child.inherit(&parent).unwrap();
    let sibling = flat();
    assert!(child.selects(INSPECT, pages(12, 1)));
    parent.invalidate();
    assert!(!child.selects(INSPECT, pages(12, 1)));
    assert!(sibling.selects(INSPECT, pages(12, 1)));
    let closed = Arc::new(AtomicBool::new(false));
    sibling.held_by(&closed).unwrap();
    closed.store(true, Ordering::Release);
    assert!(!sibling.selects(INSPECT, pages(12, 1)));
}

#[test]
fn whole_unit_bounds_and_unresolved_or_overflowing_windows_reject() {
    let grant = Pend::try_new_scope(Yes, pages(512, 1024), TRANSFER, Distributable::Yes).unwrap();
    assert!(grant.selects(TRANSFER, pages(512, 512)));
    assert!(grant.selects(TRANSFER, pages(1024, 512)));
    for scope in [
        pages(511, 512),
        pages(1025, 512),
        pages(512, 0),
        pages(usize::MAX, 2),
        PendScope::Pages(PendRange::Inherit),
    ] {
        assert!(!grant.selects(TRANSFER, scope));
    }
    grant
        .prepare_limit(Limits {
            scope: pages(512, 511),
            ..grant.limits()
        })
        .unwrap()
        .commit();
    assert!(!grant.selects(TRANSFER, pages(512, 512)));
}

#[test]
fn selection_preserves_target_kind_and_data_suspension() {
    let source = flat();
    let child = Pend::try_new_scope(
        No,
        pages(10, 8),
        Operations::Flat(FlatRights::ASSOCIATE),
        Distributable::Yes,
    )
    .unwrap();
    source.borrow_external(&child).unwrap();
    assert!(!source.selects(TRANSFER, pages(12, 1)));
    assert!(!child.selects(TRANSFER, pages(12, 1)));
    assert!(child.selects(Operations::Flat(FlatRights::ASSOCIATE), pages(12, 1)));
    assert!(!child.selects(Operations::Share(ShareRights::ASSOCIATE), pages(12, 1)));
    assert_eq!(child.modifiable(), Some(No));
}
