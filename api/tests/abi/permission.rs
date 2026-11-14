use kernel_api::model::{
    flat::FlatId,
    identity::{Mark, PendId, TaskId},
    modifiable::Modifiable,
    nest::NestId,
    nest::ReservationId,
    pend::{
        CloneRights, CreationRights, Distributable, FlatRights, Limits, NestRights, NestScope,
        Operations, PendInfo, PendRange, PendScope, PendStatus, ShareRights, Target, ViewTarget,
    },
    sign::{SignId, SignRights},
    view::ViewId,
};

fn mapping(id: u64, range: PendRange) -> PendScope {
    PendScope::Nest(NestScope::Reservation {
        id: ReservationId::from_bytes(&id.to_le_bytes()).unwrap(),
        range,
    })
}

fn grant(operations: Operations) -> Limits {
    Limits {
        operations,
        modifiable: Modifiable::No,
        distributable: Distributable::Yes,
        scope: match operations {
            Operations::Share(_) | Operations::Flat(_) => PendScope::Pages(PendRange::Inherit),
            Operations::Nest(_) => PendScope::Nest(NestScope::Whole),
            _ => PendScope::Object,
        },
    }
}

#[test]
fn reservation_migration_preserves_the_joint_target_scope_operation_contract() {
    let targets = [
        Target::None,
        Target::View(ViewTarget::Clone(ViewId::new(7))),
        Target::View(ViewTarget::Share(ViewId::new(7))),
        Target::Flat(FlatId::new(7).unwrap()),
        Target::Nest(NestId::new(7).unwrap()),
        Target::Sign(SignId::new(7)),
    ];
    let scopes = [
        PendScope::Object,
        PendScope::Pages(PendRange::Pages {
            offset: 8,
            count: 4,
        }),
        PendScope::Nest(NestScope::Whole),
        mapping(
            19,
            PendRange::Pages {
                offset: 8,
                count: 4,
            },
        ),
    ];
    // Explicit legal pairs from #142 §14. No single independently valid field
    // may make an invalid Target/Scope/Operations combination decode.
    let families = [
        (0, 0, Operations::None(CreationRights::all())),
        (1, 0, Operations::Clone(CloneRights::all())),
        (
            2,
            1,
            Operations::Share(ShareRights::ASSOCIATE | ShareRights::DELETE),
        ),
        (3, 1, Operations::Flat(FlatRights::all())),
        (4, 2, Operations::Nest(NestRights::whole())),
        (4, 3, Operations::Nest(NestRights::reservation())),
        (5, 0, Operations::Sign(SignRights::all())),
    ];
    for (family, allowed_scope, operations) in families {
        for (target_index, target) in targets.into_iter().enumerate() {
            for (scope_index, scope) in scopes.into_iter().enumerate() {
                let info = PendInfo {
                    token: PendId::mint(3),
                    target,
                    limits: Limits {
                        operations,
                        modifiable: Modifiable::No,
                        distributable: Distributable::Yes,
                        scope,
                    },
                    owner: TaskId::new(5),
                    lender: TaskId::new(6),
                    mark: Mark::NONE,
                    status: PendStatus::Active,
                };
                assert_eq!(
                    PendInfo::from_bytes(&info.to_bytes()),
                    (target_index == family && scope_index == allowed_scope).then_some(info),
                    "target {target_index}, scope {scope_index}, operations {operations:?}",
                );
            }
        }
    }
    // Reservation is a scope, not a new top-level Target. Former target tags
    // stay invalid, including a nonzero value that is a valid ReservationId.
    for tag in [5, 7, 8] {
        assert!(Target::of([tag, 19]).is_none());
    }
}

#[test]
fn each_operation_family_requires_its_own_target_and_scope() {
    let kinds = [
        (
            Target::Sign(SignId::new(7)),
            Operations::Sign(SignRights::LISTEN),
        ),
        (Target::None, Operations::None(CreationRights::FLAT)),
        (
            Target::View(ViewTarget::Clone(ViewId::new(7))),
            Operations::Clone(CloneRights::PUSH),
        ),
        (
            Target::View(ViewTarget::Share(ViewId::new(7))),
            Operations::Share(ShareRights::ASSOCIATE),
        ),
        (
            Target::Flat(FlatId::new(7).unwrap()),
            Operations::Flat(FlatRights::INSPECT),
        ),
        (
            Target::Nest(NestId::new(7).unwrap()),
            Operations::Nest(NestRights::INSPECT),
        ),
    ];
    for (i, (target, operations)) in kinds.iter().enumerate() {
        assert_eq!(Target::of(target.words()), Some(*target));
        assert!(grant(*operations).valid());
        for (j, (_, requested)) in kinds.iter().enumerate() {
            assert_eq!(target.accepts(*requested), i == j);
            assert_eq!(grant(*operations).permits_child(grant(*requested)), i == j);
        }
    }
    for words in [[0, 1], [1, 0], [3, usize::MAX], [5, 7], [7, 7], [99, 7]] {
        assert_eq!(Target::of(words), None);
    }
    for kind in [5, 7, 99] {
        for bits in [0, 1, u32::MAX as usize] {
            assert_eq!(Operations::of([kind, bits]), None);
        }
    }
}

#[test]
fn distribution_is_independent_of_mutability_and_cannot_be_restored() {
    for read in [
        grant(Operations::Share(ShareRights::ASSOCIATE)),
        Limits {
            scope: mapping(7, PendRange::Inherit),
            ..grant(Operations::Nest(NestRights::PUBLISH))
        },
    ] {
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
        assert!(!read.permits_narrowing(write));
        assert!(write.permits_narrowing(final_read));
        assert!(!write.permits_narrowing(Limits {
            distributable: Distributable::Yes,
            ..write
        }));
    }
    let read = grant(Operations::Share(ShareRights::ASSOCIATE));
    let expanded = Limits {
        operations: Operations::Share(ShareRights::ASSOCIATE | ShareRights::DELETE),
        ..read
    };
    assert!(!read.permits_child(expanded));
}

#[test]
fn scopes_exclude_incompatible_families_and_invalid_page_geometry() {
    let scopes = [
        PendScope::Object,
        PendScope::Pages(PendRange::Inherit),
        PendScope::Pages(PendRange::pages(2, 3).unwrap()),
        PendScope::Nest(NestScope::Whole),
        mapping(7, PendRange::Inherit),
        mapping(7, PendRange::pages(2, 3).unwrap()),
    ];
    for (operations, expected) in [
        (
            Operations::None(CreationRights::FLAT),
            [true, false, false, false, false, false],
        ),
        (
            Operations::Clone(CloneRights::PUSH),
            [true, false, false, false, false, false],
        ),
        (
            Operations::Sign(SignRights::LISTEN),
            [true, false, false, false, false, false],
        ),
        (
            Operations::Share(ShareRights::ASSOCIATE),
            [false, true, true, false, false, false],
        ),
        (
            Operations::Flat(FlatRights::INSPECT),
            [false, true, true, false, false, false],
        ),
        (
            Operations::Nest(NestRights::INSPECT),
            [false, false, false, true, true, true],
        ),
    ] {
        for (scope, expected) in scopes.into_iter().zip(expected) {
            let limits = Limits {
                scope,
                ..grant(operations)
            };
            assert_eq!(limits.valid(), expected, "{operations:?} {scope:?}");
            assert_eq!(Limits::of(limits.words()), expected.then_some(limits));
        }
    }
    for range in [
        PendRange::Pages {
            offset: 0,
            count: 0,
        },
        PendRange::Pages {
            offset: usize::MAX,
            count: 1,
        },
        PendRange::Pages {
            offset: 0,
            count: usize::MAX,
        },
    ] {
        for scope in [PendScope::Pages(range), mapping(7, range)] {
            assert_eq!(PendScope::of(scope.words()), None);
        }
    }
    for operations in [
        Operations::None(CreationRights::all()),
        Operations::Sign(SignRights::all()),
    ] {
        let invalid = Limits {
            modifiable: Modifiable::Yes,
            ..grant(operations)
        };
        assert!(!invalid.valid());
        assert_eq!(Limits::of(invalid.words()), None);
    }
}

#[test]
fn scope_codec_is_five_canonical_words_with_a_checked_mapping_identity() {
    assert_eq!(PendScope::WORDS, 5);
    assert_eq!(Limits::WORDS, 9);
    for (scope, words) in [
        (PendScope::Object, [0, 0, 0, 0, 0]),
        (PendScope::Pages(PendRange::Inherit), [1, 0, 0, 0, 0]),
        (
            PendScope::Pages(PendRange::pages(2, 3).unwrap()),
            [1, 0, 1, 2, 3],
        ),
        (PendScope::Nest(NestScope::Whole), [2, 0, 0, 0, 0]),
        (mapping(7, PendRange::Inherit), [3, 7, 0, 0, 0]),
        (
            mapping(isize::MAX as u64, PendRange::pages(2, 3).unwrap()),
            [3, isize::MAX as usize, 1, 2, 3],
        ),
    ] {
        assert_eq!(scope.words(), words);
        assert_eq!(PendScope::of(words), Some(scope));
        assert_eq!(
            scope.range(),
            match scope {
                PendScope::Pages(range) | PendScope::Nest(NestScope::Reservation { range, .. }) =>
                    Some(range),
                _ => None,
            }
        );
    }
    for invalid in [
        [0, 1, 0, 0, 0],
        [0, 0, 1, 0, 1],
        [0, 0, 0, 1, 0],
        [0, 0, 0, 0, 1],
        [1, 1, 0, 0, 0],
        [1, 0, 0, 1, 0],
        [1, 0, 0, 0, 1],
        [1, 0, 2, 0, 1],
        [1, 0, 1, 0, 0],
        [1, 0, 1, usize::MAX, 1],
        [1, 0, 1, 0, usize::MAX],
        [2, 1, 0, 0, 0],
        [2, 0, 1, 0, 1],
        [2, 0, 0, 1, 0],
        [2, 0, 0, 0, 1],
        [3, 0, 0, 0, 0],
        [3, usize::MAX, 0, 0, 0],
        [3, 7, 0, 1, 0],
        [3, 7, 0, 0, 1],
        [3, 7, 2, 0, 1],
        [3, 7, 1, 0, 0],
        [3, 7, 1, usize::MAX, 1],
        [3, 7, 1, 0, usize::MAX],
        [4, 0, 0, 0, 0],
    ] {
        assert_eq!(PendScope::of(invalid), None, "{invalid:?}");
    }
    let invalid = Limits {
        scope: PendScope::Nest(NestScope::Reservation {
            id: ReservationId::NONE,
            range: PendRange::Inherit,
        }),
        ..grant(Operations::Nest(NestRights::INSPECT))
    };
    assert!(!invalid.valid());
}

#[test]
fn whole_and_mapping_rights_have_distinct_masks_and_retired_bits_stay_invalid() {
    assert_eq!(NestRights::whole().bits(), 1 | 2 | 8 | 512);
    assert_eq!(
        NestRights::reservation().bits(),
        1 | 4 | 64 | 128 | 256 | 1024 | 2048 | 4096
    );
    assert_eq!(
        NestRights::all(),
        NestRights::whole() | NestRights::reservation()
    );
    for (scope, mask) in [
        (PendScope::Nest(NestScope::Whole), NestRights::whole()),
        (mapping(7, PendRange::Inherit), NestRights::reservation()),
    ] {
        for bit in 0..13 {
            let bits = 1u32 << bit;
            let rights = NestRights::from_bits_retain(bits);
            let limits = Limits {
                scope,
                ..grant(Operations::Nest(rights))
            };
            assert_eq!(limits.valid(), mask.contains(rights), "{scope:?} bit {bit}");
        }
        let all = Limits {
            scope,
            ..grant(Operations::Nest(NestRights::all()))
        };
        assert!(!all.valid());
        assert_eq!(Limits::of(all.words()), None);
    }
    for bits in [16, 32, 8192, u32::MAX] {
        assert!(NestRights::from_bits(bits).is_none());
        assert!(Operations::of([4, bits as usize]).is_none());
    }
}

#[test]
fn generic_attenuation_never_changes_scope_category_or_mapping_identity() {
    let whole = grant(Operations::Nest(NestRights::INSPECT));
    let first = Limits {
        scope: mapping(7, PendRange::pages(2, 3).unwrap()),
        ..whole
    };
    let second = Limits {
        scope: mapping(8, PendRange::Inherit),
        ..first
    };
    for (parent, child) in [
        (whole, first),
        (first, whole),
        (first, second),
        (second, first),
    ] {
        assert!(parent.valid() && child.valid());
        assert!(!parent.permits_child(child));
        assert!(!parent.permits_narrowing(child));
    }
    for scope in [
        mapping(7, PendRange::Inherit),
        mapping(7, PendRange::pages(1, 1).unwrap()),
    ] {
        let child = Limits { scope, ..first };
        assert!(first.permits_child(child));
        assert!(first.permits_narrowing(child));
    }
}

#[test]
fn info_requires_resolved_scope_and_matching_target_family() {
    let info = PendInfo {
        token: PendId::mint(1),
        target: Target::Nest(NestId::new(3).unwrap()),
        limits: grant(Operations::Nest(NestRights::RESERVE)),
        owner: TaskId::new(5),
        lender: TaskId::new(0),
        mark: Mark::NONE,
        status: PendStatus::Active,
    };
    assert_eq!(PendInfo::WORDS, 16);
    assert_eq!(PendInfo::SIZE, 128);
    assert_eq!(PendInfo::from_bytes(&info.to_bytes()), Some(info));
    for limits in [
        grant(Operations::Flat(FlatRights::INSPECT)),
        Limits {
            scope: mapping(7, PendRange::Inherit),
            ..grant(Operations::Nest(NestRights::INSPECT))
        },
        Limits {
            scope: PendScope::Object,
            ..info.limits
        },
        grant(Operations::Nest(NestRights::PUBLISH)),
    ] {
        assert_eq!(
            PendInfo::from_bytes(&PendInfo { limits, ..info }.to_bytes()),
            None
        );
    }
    let mapped = PendInfo {
        limits: Limits {
            scope: mapping(7, PendRange::pages(20, 3).unwrap()),
            ..grant(Operations::Nest(NestRights::INSPECT))
        },
        ..info
    };
    assert_eq!(PendInfo::from_bytes(&mapped.to_bytes()), Some(mapped));
    let flat = PendInfo {
        target: Target::Flat(FlatId::new(3).unwrap()),
        limits: Limits {
            scope: PendScope::Pages(PendRange::pages(20, 3).unwrap()),
            ..grant(Operations::Flat(FlatRights::INSPECT))
        },
        ..info
    };
    assert_eq!(PendInfo::from_bytes(&flat.to_bytes()), Some(flat));
    let inherited = PendInfo {
        limits: grant(flat.limits.operations),
        ..flat
    };
    assert_eq!(PendInfo::from_bytes(&inherited.to_bytes()), None);
}

#[test]
fn observation_and_access_cannot_supply_mutation_authority() {
    assert!(
        !Operations::Share(ShareRights::ASSOCIATE | ShareRights::DELETE)
            .contains(Operations::Flat(FlatRights::TRANSFER))
    );
    assert!(!Operations::Flat(FlatRights::ASSOCIATE).contains(Operations::Flat(FlatRights::BLOCK)));
    let inspect = Limits {
        scope: mapping(7, PendRange::Inherit),
        ..grant(Operations::Nest(NestRights::INSPECT))
    };
    for operation in [
        NestRights::INSERT,
        NestRights::REMOVE,
        NestRights::PUBLISH,
        NestRights::RETRACT,
        NestRights::PROTECT,
        NestRights::RELEASE,
    ] {
        assert!(!inspect.permits_child(Limits {
            operations: Operations::Nest(operation),
            ..inspect
        }));
    }
    let listen = grant(Operations::Sign(SignRights::LISTEN));
    assert!(grant(Operations::Sign(SignRights::all())).permits_child(listen));
    assert!(!listen.permits_child(grant(Operations::Sign(SignRights::CONSUME))));
    assert!(!listen.permits_child(grant(Operations::Sign(SignRights::DELETE))));
}
