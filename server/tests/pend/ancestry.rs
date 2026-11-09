use crate::authority::Pend;
use kernel_api::{
    abi::call::pend::PendFail,
    model::{
        modifiable::Modifiable::{No, Yes},
        pend::{CreationRights, Distributable, Operations, PendStatus},
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
