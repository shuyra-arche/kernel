use kernel_api::model::{
    flat::FlatId,
    identity::{Mark, PendId, TaskId},
    modifiable::Modifiable,
    nest::NestId,
    pend::{
        CloneRights, CreationRights, Distributable, FlatRights, Limits, NestRights, Operations,
        PendInfo, PendRange, PendStatus, PortRights, ShareRights, Target, ViewTarget,
    },
    port::PortId,
    view::ViewId,
};

fn grant(operations: Operations) -> Limits {
    Limits {
        operations,
        modifiable: Modifiable::No,
        distributable: Distributable::Yes,
        range: PendRange::Inherit,
    }
}

#[test]
fn each_operation_family_requires_its_own_target() {
    let kinds = [
        (Target::None, Operations::None(CreationRights::FLAT)),
        (
            Target::View(ViewTarget::Clone(ViewId::new(7))),
            Operations::Clone(CloneRights::PUSH),
        ),
        (
            Target::View(ViewTarget::Share(ViewId::new(7))),
            Operations::Share(ShareRights::OPEN),
        ),
        (
            Target::Flat(FlatId::new(7).unwrap()),
            Operations::Flat(FlatRights::INSPECT),
        ),
        (
            Target::Nest(NestId::new(7).unwrap()),
            Operations::Nest(NestRights::INSPECT),
        ),
        (
            Target::Port(PortId::new(7).unwrap()),
            Operations::Port(PortRights::INSPECT),
        ),
    ];
    for (i, (target, operations)) in kinds.iter().enumerate() {
        assert_eq!(Target::of(target.words()), Some(*target));
        for (j, (_, requested)) in kinds.iter().enumerate() {
            assert_eq!(target.accepts(*requested), i == j);
            assert_eq!(grant(*operations).permits_child(grant(*requested)), i == j);
        }
    }
    for words in [[0, 1], [1, 0], [3, usize::MAX], [99, 7]] {
        assert_eq!(Target::of(words), None);
    }
}

#[test]
fn distribution_is_independent_of_mutability_and_cannot_be_restored() {
    let read = grant(Operations::Share(ShareRights::OPEN));
    let final_read = Limits {
        distributable: Distributable::No,
        ..read
    };
    assert!(read.permits_child(final_read));
    assert!(!final_read.permits_child(final_read));
    assert!(read.permits_narrowing(final_read));
    assert!(!final_read.permits_narrowing(read));
    let write = Limits {
        modifiable: Modifiable::Yes,
        ..final_read
    };
    assert!(!write.permits_child(final_read));
    assert!(!read.permits_child(write));
    assert!(write.permits_narrowing(final_read));
    let expanded = Limits {
        operations: Operations::Share(ShareRights::OPEN | ShareRights::ASSOCIATE),
        ..final_read
    };
    assert!(!read.permits_child(expanded));
}

#[test]
fn object_free_permission_never_admits_a_data_window_or_mutable_access() {
    let calls = grant(Operations::None(
        CreationRights::FLAT | CreationRights::NEST,
    ));
    assert!(calls.valid());
    for invalid in [
        Limits {
            range: PendRange::Pages {
                offset: 0,
                count: 1,
            },
            ..calls
        },
        Limits {
            modifiable: Modifiable::Yes,
            ..calls
        },
    ] {
        assert!(!invalid.valid());
        assert_eq!(Limits::of(invalid.words()), None);
    }
}

#[test]
fn info_rejects_a_valid_permission_for_the_wrong_target() {
    let info = PendInfo {
        token: PendId::mint(1),
        target: Target::Nest(NestId::new(3).unwrap()),
        limits: grant(Operations::Nest(NestRights::RESERVE)),
        owner: TaskId::new(5),
        lender: TaskId::new(0),
        mark: Mark::NONE,
        status: PendStatus::Active,
    };
    assert_eq!(PendInfo::from_bytes(&info.to_bytes()), Some(info));
    let forged = PendInfo {
        limits: grant(Operations::Flat(FlatRights::INSPECT)),
        ..info
    };
    assert_eq!(PendInfo::from_bytes(&forged.to_bytes()), None);
}
