use crate::authority::Pend;
use kernel_api::{
    abi::call::pend::PendFail,
    model::{
        modifiable::Modifiable::{No, Yes},
        pend::{CreationRights, Distributable, Limits, NestRights, Operations, PendStatus},
        sign::SignRights,
    },
};
use std::sync::Arc;
fn root(operations: Operations) -> Arc<Pend> {
    Pend::try_new_window(No, None, operations, Distributable::Yes).unwrap()
}
#[test]
fn logical_closure_rejects_unvisited_descendants_and_keeps_siblings_alive() {
    for operations in [
        Operations::None(CreationRights::all()),
        Operations::Sign(SignRights::all()),
        Operations::Nest(NestRights::all()),
    ] {
        let parent = root(operations);
        let branch = root(operations);
        branch.inherit(&parent).unwrap();
        let leaf = root(operations);
        leaf.inherit(&branch).unwrap();
        let sibling = root(operations);
        sibling.inherit(&parent).unwrap();
        assert!(leaf.permits(operations));
        branch.invalidate();
        assert!(!leaf.alive());
        assert!(!leaf.permits(operations));
        assert_eq!(leaf.modifiable(), None);
        assert_eq!(leaf.status(), PendStatus::Retiring);
        assert_eq!(leaf.retain_route(), Err(PendFail::Dead));
        assert_eq!(leaf.can_borrow(No), Err(PendFail::Dead));
        assert!(sibling.permits(operations));
        assert!(parent.permits(operations));
    }
}
#[test]
fn ancestry_is_immutable_acyclic_and_has_an_admission_bound() {
    let operations = Operations::None(CreationRights::all());
    let first = root(operations);
    assert_eq!(first.inherit(&first), Err(PendFail::Denied));
    let mut last = first.clone();
    for _ in 1..64 {
        let child = root(operations);
        child.inherit(&last).unwrap();
        last = child;
    }
    assert_eq!(root(operations).inherit(&last), Err(PendFail::Denied));
    assert_eq!(first.inherit(&last), Err(PendFail::Denied));
    assert_eq!(last.inherit(&root(operations)), Err(PendFail::Denied));
    first.invalidate();
    assert!(!last.alive());
}
#[test]
fn logical_closure_does_not_return_a_hardware_borrow() {
    let operations = Operations::None(CreationRights::empty());
    let parent = Pend::try_new_window(Yes, None, operations, Distributable::Yes).unwrap();
    let leaf = root(operations);
    parent.borrow_external(&leaf).unwrap();
    parent.invalidate();
    assert!(parent.external_pending());
    assert_eq!(parent.settle(), Err(PendFail::Busy));
    leaf.invalidate();
    leaf.settle().unwrap();
    assert!(!parent.external_pending());
    parent.settle().unwrap();
    assert!(!parent.alive());
}

#[test]
fn ledger_closure_invalidates_unvisited_descendants_without_reopening_a_borrow() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let operations = Operations::Sign(SignRights::all());
    let gate = Arc::new(AtomicBool::new(false));
    let parent = root(operations);
    parent.held_by(&gate).unwrap();
    let leaf = root(operations);
    leaf.inherit(&parent).unwrap();
    assert!(leaf.permits(operations));
    gate.store(true, Ordering::Release);
    assert!(!leaf.permits(operations));
    assert_eq!(leaf.retain_route(), Err(PendFail::Dead));
    assert_eq!(
        parent.held_by(&Arc::new(AtomicBool::new(false))),
        Err(PendFail::Denied)
    );
}

#[test]
fn retirement_retention_does_not_freeze_narrowing_or_restore_authority() {
    let parent = root(Operations::Nest(NestRights::all()));
    parent.retain_retirement().unwrap();
    let limit = Limits {
        operations: Operations::Nest(NestRights::INSPECT),
        distributable: Distributable::No,
        ..parent.limits()
    };
    assert_eq!(parent.can_narrow(limit), Ok(()));
    parent.narrow(limit, None).unwrap();
    assert!(parent.permits(limit.operations));
    assert!(!parent.permits(Operations::Nest(NestRights::PUBLISH)));
    assert!(!parent.limits().permits_child(limit));
    parent.invalidate();
    assert_eq!(parent.status(), PendStatus::Retiring);
    assert_eq!(parent.settle(), Err(PendFail::Busy));
    assert_eq!(parent.retain_retirement(), Err(PendFail::Dead));
    // Host supplies a logical discharge; actual hardware proof is exercised
    // by the IOMMU backend and Kernel guest tests, not by this state test.
    unsafe { parent.release_retirement() };
    assert!(!parent.external_pending());
    parent.settle().unwrap();
    assert_eq!(parent.status(), PendStatus::Dead);
    assert!(!parent.permits(limit.operations));
}

#[test]
fn publication_permission_cannot_narrow_until_its_exact_route_returns() {
    let branch = root(Operations::Nest(NestRights::all()));
    branch.retain_retirement().unwrap();
    branch.retain_route().unwrap();
    let limit = Limits {
        operations: Operations::Nest(NestRights::INSPECT),
        ..branch.limits()
    };
    assert_eq!(branch.can_narrow(limit), Err(PendFail::Busy));
    assert_eq!(branch.narrow(limit, None), Err(PendFail::Busy));
    unsafe { branch.release_route() };
    branch.narrow(limit, None).unwrap();
    assert!(branch.external_pending());
    branch.invalidate();
    assert_eq!(branch.settle(), Err(PendFail::Busy));
    unsafe { branch.release_retirement() };
    branch.settle().unwrap();
}

#[test]
fn child_retirement_does_not_discharge_root_or_sibling_obligations() {
    let operations = Operations::Nest(NestRights::all());
    let parent = root(operations);
    parent.retain_retirement().unwrap();
    let first = root(operations);
    first.inherit(&parent).unwrap();
    let other = root(operations);
    other.inherit(&parent).unwrap();
    first.retain_route().unwrap();
    other.retain_route().unwrap();
    first.invalidate();
    assert_eq!(first.settle(), Err(PendFail::Busy));
    assert!(parent.permits(operations));
    assert!(other.permits(operations));
    unsafe { first.release_route() };
    first.settle().unwrap();
    assert!(parent.external_pending());
    assert!(other.external_pending());
    assert!(other.permits(operations));
    parent.invalidate();
    assert!(!other.permits(operations));
    other.invalidate();
    assert_eq!(other.settle(), Err(PendFail::Busy));
    unsafe { other.release_route() };
    other.settle().unwrap();
    assert_eq!(parent.settle(), Err(PendFail::Busy));
    unsafe { parent.release_retirement() };
    parent.settle().unwrap();
}
