//! Production transfer ceilings and real Pend ancestry. These tests neither
//! manufacture hardware settlement nor replace provenance checks with booleans.
use crate::authority::{Carried, Pend};
use kernel_api::{
    abi::{call::pend::PendFail, memory::rights::VACATE},
    address::{PageCount, PageOffset},
    model::{
        identity::TaskId,
        modifiable::Modifiable,
        pend::{Distributable, FlatRights, Operations, PendRange, PendScope},
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
fn grant(rights: FlatRights, access: Modifiable, distribution: Distributable) -> Arc<Pend> {
    Pend::try_new_scope(
        access,
        PendScope::Pages(PendRange::Pages {
            offset: 2,
            count: 4,
        }),
        Operations::Flat(rights),
        distribution,
    )
    .unwrap()
}
fn carry(pend: &Arc<Pend>) -> Carried {
    Carried::new(
        pend,
        TaskId::new(10),
        PageOffset::new(2),
        PageCount::new(2).unwrap(),
    )
    .unwrap()
}
#[test]
fn transfer_only_admits_movement_without_read_write_release_or_export() {
    let branch = grant(FlatRights::TRANSFER, Modifiable::No, Distributable::No);
    let carried = carry(&branch);
    assert!(carried.live());
    assert!(carried.permits(TaskId::new(10), FlatRights::TRANSFER, None, false));
    assert!(!carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        false
    ));
    assert!(!carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::Yes),
        false
    ));
    assert!(!carried.permits(TaskId::new(10), VACATE, None, false));
    assert!(!carried.permits(TaskId::new(10), FlatRights::TRANSFER, None, true));
    let inspect = grant(FlatRights::INSPECT, Modifiable::Yes, Distributable::Yes);
    assert!(matches!(
        Carried::new(
            &inspect,
            TaskId::new(10),
            PageOffset::new(2),
            PageCount::new(2).unwrap()
        ),
        Err(PendFail::Denied)
    ));
}
#[test]
fn read_only_non_distributable_origin_does_not_inherit_destination_privileges() {
    let branch = grant(
        FlatRights::TRANSFER | FlatRights::ASSOCIATE | VACATE,
        Modifiable::No,
        Distributable::No,
    );
    let carried = carry(&branch);
    assert!(carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        false
    ));
    assert!(!carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::Yes),
        false
    ));
    assert!(!carried.permits(
        TaskId::new(11),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        false
    ));
    assert!(!carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        true
    ));
    // Data-write permission and physical disposal are independent.
    assert!(carried.permits(TaskId::new(10), VACATE, None, false));
    assert!(!carried.permits(TaskId::new(11), VACATE, None, false));
}
#[test]
fn narrowing_original_branch_is_observed_without_replacing_the_carried_record() {
    let branch = grant(
        FlatRights::TRANSFER | FlatRights::ASSOCIATE,
        Modifiable::Yes,
        Distributable::Yes,
    );
    let carried = carry(&branch);
    assert!(carried.permits(
        TaskId::new(11),
        FlatRights::ASSOCIATE,
        Some(Modifiable::Yes),
        true
    ));
    let mut limits = branch.limits();
    limits.modifiable = Modifiable::No;
    limits.distributable = Distributable::No;
    limits.operations = Operations::Flat(FlatRights::TRANSFER | FlatRights::ASSOCIATE);
    branch.narrow(limits).unwrap();
    assert!(carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        false
    ));
    assert!(!carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::Yes),
        false
    ));
    assert!(!carried.permits(TaskId::new(10), VACATE, None, false));
    assert!(!carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        true
    ));
    limits.scope = PendScope::Pages(PendRange::Pages {
        offset: 4,
        count: 2,
    });
    branch.narrow(limits).unwrap();
    assert!(!carried.live());
}
#[test]
fn recovery_requires_real_ancestor_and_preserves_all_original_ceilings() {
    let root = grant(
        FlatRights::TRANSFER | FlatRights::ASSOCIATE | VACATE,
        Modifiable::Yes,
        Distributable::Yes,
    );
    let leaf = grant(
        FlatRights::TRANSFER | FlatRights::ASSOCIATE,
        Modifiable::No,
        Distributable::No,
    );
    leaf.inherit(&root).unwrap();
    let dead = Arc::new(AtomicBool::new(false));
    leaf.held_by(&dead).unwrap();
    let mut carried = carry(&leaf);
    dead.store(true, Ordering::Release);
    assert!(!carried.live());
    let sibling = grant(
        FlatRights::TRANSFER | FlatRights::ASSOCIATE | VACATE,
        Modifiable::Yes,
        Distributable::Yes,
    );
    sibling.inherit(&root).unwrap();
    carried.recover(TaskId::new(11), &sibling);
    assert!(!carried.live());
    carried.recover(TaskId::new(11), &root);
    assert!(carried.live());
    assert!(carried.permits(
        TaskId::new(11),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        false
    ));
    assert!(!carried.permits(
        TaskId::new(11),
        FlatRights::ASSOCIATE,
        Some(Modifiable::Yes),
        false
    ));
    assert!(!carried.permits(
        TaskId::new(11),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        true
    ));
    assert!(!carried.permits(TaskId::new(11), VACATE, None, false));
    assert!(!carried.permits(
        TaskId::new(10),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        false
    ));
}
#[test]
fn restoring_original_position_requires_exact_geometry_and_ancestry() {
    let root = grant(FlatRights::TRANSFER, Modifiable::Yes, Distributable::Yes);
    let leaf = grant(FlatRights::TRANSFER, Modifiable::No, Distributable::No);
    leaf.inherit(&root).unwrap();
    let sibling = grant(FlatRights::TRANSFER, Modifiable::Yes, Distributable::Yes);
    sibling.inherit(&root).unwrap();
    let carried = carry(&leaf);
    let count = PageCount::new(2).unwrap();
    assert!(!carried.returns_to(&sibling, PageOffset::new(2), count));
    assert!(!carried.returns_to(&root, PageOffset::new(3), count));
    assert!(!carried.returns_to(&root, PageOffset::new(2), PageCount::new(1).unwrap()));
    assert!(carried.returns_to(&root, PageOffset::new(2), count));
    root.invalidate();
    assert!(!carried.returns_to(&root, PageOffset::new(2), count));
}

#[test]
fn takeover_cannot_restore_permissions_narrowed_after_the_transfer() {
    let root = grant(
        FlatRights::TRANSFER | FlatRights::ASSOCIATE,
        Modifiable::Yes,
        Distributable::Yes,
    );
    let leaf = grant(
        FlatRights::TRANSFER | FlatRights::ASSOCIATE,
        Modifiable::Yes,
        Distributable::Yes,
    );
    leaf.inherit(&root).unwrap();
    let mut carried = carry(&leaf);
    let mut limits = leaf.limits();
    limits.modifiable = Modifiable::No;
    limits.distributable = Distributable::No;
    limits.operations = Operations::Flat(FlatRights::TRANSFER);
    leaf.narrow(limits).unwrap();
    leaf.invalidate();
    carried.recover(TaskId::new(11), &root);
    assert!(carried.live());
    assert!(!carried.permits(
        TaskId::new(11),
        FlatRights::ASSOCIATE,
        Some(Modifiable::No),
        false
    ));
    assert!(!carried.permits(
        TaskId::new(11),
        FlatRights::TRANSFER,
        Some(Modifiable::Yes),
        false
    ));
    assert!(!carried.permits(TaskId::new(11), FlatRights::TRANSFER, None, true));
}
