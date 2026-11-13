//! Actual maintenance epoch and Pend ancestry. Task lifetime is the existing
//! registration fixture; no test here manufactures hardware settlement.
use crate::{authority::Pend, epoch::Epoch, support::Task};
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::{
        flat::{BlockGeneration, BlockPhase, BlockRef, FlatId},
        identity::PendId,
        modifiable::Modifiable,
        pend::{Distributable, FlatRights, Operations, PendRange, PendScope},
    },
};
use std::sync::{Arc, atomic::Ordering};
fn grant(access: Modifiable, offset: usize, count: usize) -> Arc<Pend> {
    Pend::try_new_scope(
        access,
        PendScope::Pages(PendRange::Pages { offset, count }),
        Operations::Flat(FlatRights::BLOCK | FlatRights::ASSOCIATE | FlatRights::TRANSFER),
        Distributable::Yes,
    )
    .unwrap()
}
fn token(n: usize) -> PendId {
    PendId::mint(n)
}
fn reference() -> BlockRef {
    BlockRef::new(FlatId::new(1).unwrap(), 7, BlockGeneration::INITIAL).unwrap()
}
#[test]
fn takeover_requires_dead_owner_actual_ancestor_and_whole_original_window() {
    let parent = grant(Modifiable::Yes, 0, 8);
    let child = grant(Modifiable::No, 2, 4);
    child.inherit(&parent).unwrap();
    let sibling = grant(Modifiable::Yes, 0, 8);
    sibling.inherit(&parent).unwrap();
    let worker = Task::new(1);
    let manager = Task::new(2);
    let mut epoch = Epoch::new(reference(), &worker, token(1), &child, 0);
    let window = (PageOffset::new(2), PageCount::new(4).unwrap());
    assert_eq!(
        epoch.takeover(reference(), &manager, token(2), &parent, window),
        Err(PendFail::Denied)
    );
    worker.dead.store(true, Ordering::Release);
    assert!(!epoch.live(window));
    assert_eq!(
        epoch.takeover(reference(), &manager, token(2), &sibling, window),
        Err(PendFail::Denied)
    );
    let unrelated = grant(Modifiable::Yes, 0, 8);
    assert_eq!(
        epoch.takeover(reference(), &manager, token(2), &unrelated, window),
        Err(PendFail::Denied)
    );
    epoch
        .takeover(reference(), &manager, token(2), &parent, window)
        .unwrap();
    assert_eq!(epoch.reference.generation().get(), 2);
    assert_eq!(epoch.phase, BlockPhase::Draining);
    assert_eq!(epoch.data, Modifiable::No); // takeover cannot wash the old ceiling
    assert!(epoch.live(window));
    assert_eq!(
        epoch.takeover(reference(), &manager, token(2), &parent, window),
        Err(PendFail::Denied)
    );
}
#[test]
fn narrowed_manager_and_exhausted_epoch_preserve_original_record() {
    let manager = Task::new(3);
    let worker = Task::new(4);
    let parent = grant(Modifiable::Yes, 2, 1);
    let child = grant(Modifiable::No, 0, 8);
    child.inherit(&parent).unwrap(); // ancestry alone is not range authorization
    let window = (PageOffset::new(2), PageCount::new(4).unwrap());
    let mut epoch = Epoch::new(reference(), &worker, token(3), &child, 0);
    worker.dead.store(true, Ordering::Release);
    assert_eq!(
        epoch.takeover(reference(), &manager, token(4), &parent, window),
        Err(PendFail::Denied)
    );
    assert_eq!(epoch.reference, reference());
    let full = grant(Modifiable::Yes, 0, 8);
    let leaf = grant(Modifiable::No, 0, 8);
    leaf.inherit(&full).unwrap();
    let last = BlockRef::new(
        FlatId::new(1).unwrap(),
        9,
        BlockGeneration::new(u64::MAX).unwrap(),
    )
    .unwrap();
    let mut epoch = Epoch::new(last, &worker, token(3), &leaf, 0);
    assert_eq!(
        epoch.takeover(last, &manager, token(4), &full, window),
        Err(PendFail::OutOfMemory)
    );
    assert_eq!(epoch.reference, last);
    assert!(Arc::ptr_eq(&epoch.authority, &leaf));
}

#[test]
fn narrowed_owner_loses_credential_before_task_death() {
    let parent = grant(Modifiable::Yes, 0, 8);
    let child = grant(Modifiable::No, 0, 8);
    child.inherit(&parent).unwrap();
    let worker = Task::new(10);
    let manager = Task::new(11);
    let window = (PageOffset::new(2), PageCount::new(4).unwrap());
    let mut epoch = Epoch::new(reference(), &worker, token(10), &child, 0);
    assert!(epoch.live(window));
    let mut limits = child.limits();
    limits.scope = PendScope::Pages(PendRange::Pages {
        offset: 2,
        count: 1,
    });
    child.narrow(limits).unwrap();
    assert!(!worker.dead.load(Ordering::Acquire));
    assert!(!epoch.live(window));
    epoch
        .takeover(reference(), &manager, token(11), &parent, window)
        .unwrap();
    assert_eq!(epoch.reference.generation().get(), 2);
    assert_eq!(epoch.data, Modifiable::No);
}

#[test]
fn removing_block_right_or_revoking_branch_invalidates_live_credential() {
    let owner = Task::new(12);
    let branch = grant(Modifiable::Yes, 0, 8);
    let window = (PageOffset::ZERO, PageCount::new(8).unwrap());
    let epoch = Epoch::new(reference(), &owner, token(12), &branch, 0);
    let mut limits = branch.limits();
    limits.operations = Operations::Flat(FlatRights::ASSOCIATE);
    branch.narrow(limits).unwrap();
    assert!(branch.alive());
    assert!(!epoch.live(window));
    let other = grant(Modifiable::Yes, 0, 8);
    let revoked = Epoch::new(reference(), &owner, token(13), &other, 0);
    other.invalidate();
    assert!(!revoked.live(window));
}
