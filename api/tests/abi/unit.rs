use kernel_api::{
    abi::register::Wire,
    address::VirtAddr,
    model::{
        identity::{TaskId, TeamId},
        unit::{
            ControlId, CreatedUnit, ExitCause, TaskStart, UnitCreate, UnitInfo, UnitQuery,
            UnitState, UnitTarget,
        },
    },
};
#[test]
fn creation_variants_use_checked_fixed_width_snapshots() {
    for request in [
        UnitCreate::Team {
            nest: None,
            guard: ControlId::NONE,
            member_exit: false,
        },
        UnitCreate::Task {
            team: Some(TeamId::new(9)),
            start: TaskStart {
                entry: VirtAddr::new(0x4000),
                arguments: VirtAddr::new(0x8000),
                argument_count: 2,
                stack_pointer: VirtAddr::new(0x9000),
            },
        },
        UnitCreate::Team {
            nest: Some(kernel_api::model::identity::PendId::mint(9)),
            guard: ControlId::NONE,
            member_exit: true,
        },
    ] {
        let bytes = request.to_bytes();
        assert_eq!(UnitCreate::from_bytes(&bytes), Some(request));
        assert!(UnitCreate::from_bytes(&bytes[..47]).is_none());
    }
    let mut team = UnitCreate::Team {
        nest: None,
        guard: ControlId::NONE,
        member_exit: false,
    }
    .to_bytes();
    team[32] = 2;
    assert!(UnitCreate::from_bytes(&team).is_none());
    team[32] = 0;
    // Both ABI selections retain the width and canonical User encoding. The
    // old Supervisor selector and every nonzero reserved word fail decoding.
    for retired in [1u64, 2, 255, u64::MAX] {
        team[8..16].copy_from_slice(&retired.to_le_bytes());
        assert!(UnitCreate::from_bytes(&team).is_none());
    }
}
#[test]
fn query_and_result_encoding_preserve_identity_and_independent_cleanup() {
    for query in [
        UnitQuery::Caller,
        UnitQuery::Target(UnitTarget::Task(TaskId::new(7))),
        UnitQuery::Children {
            after: TeamId::new(0),
        },
        UnitQuery::Members {
            team: TeamId::new(3),
            after: TaskId::new(0),
        },
    ] {
        let mut words = [0; 6];
        query.pack(&mut words, &mut 0);
        assert_eq!(UnitQuery::unpack(&words, &mut 0).unwrap(), query);
    }
    for bad in [
        [0, 1, 0, 0, 0, 0],
        [1, 0, 0, 0, 0, 0],
        [1, 2, 1, 0, 0, 0],
        [4, 0, 0, 0, 0, 0],
    ] {
        assert!(UnitQuery::unpack(&bad, &mut 0).is_err());
    }
    let record = UnitInfo {
        target: UnitTarget::Task(TaskId::new(7)),
        team: TeamId::new(3),
        controller: TaskId::new(1),
        creator: TaskId::new(1),
        sire: TaskId::new(1),
        state: UnitState::Exited,
        reclaimed: false,
        admitting: false,
        exit: Some((ExitCause::Reap, 17)),
        result: Some(u64::MAX),
    };
    let bytes = record.to_bytes();
    assert_eq!(UnitInfo::from_bytes(&bytes), Some(record));
    for index in [0, 56, 64, 88, 104] {
        let mut bad = bytes;
        bad[index] = 9;
        assert!(UnitInfo::from_bytes(&bad).is_none());
    }
}

#[test]
fn create_reply_preserves_team_nest_authority_and_rejects_malformed_triples() {
    use kernel_api::{
        abi::{call::unit::operations::Create, operation::KernelCall},
        model::identity::PendId,
    };
    let task = CreatedUnit::Task(TaskId::new(7));
    let team = CreatedUnit::Team {
        team: TeamId::new(8),
        nest: PendId::mint(9),
    };
    for (words, expected) in [((0, 7, 0), task), ((1, 8, 9), team)] {
        assert_eq!(
            CreatedUnit::decode(words.0, words.1, words.2),
            Some(expected)
        );
        assert_eq!(Create::decode_return(words).unwrap(), expected);
    }
    for (kind, id, nest) in [
        (2, 7, 0),
        (2, 8, 9),
        (0, 0, 0),
        (1, 0, 9),
        (0, usize::MAX, 0),
        (1, usize::MAX, 9),
        (0, 7, 9),
        (1, 8, 0),
        (1, 8, usize::MAX),
    ] {
        assert_eq!(CreatedUnit::decode(kind, id, nest), None);
        assert!(std::panic::catch_unwind(|| Create::decode_return((kind, id, nest))).is_err());
    }
    assert_eq!(
        CreatedUnit::decode(0, isize::MAX as usize, 0),
        Some(CreatedUnit::Task(TaskId::new(isize::MAX as usize)))
    );
    assert!(matches!(
        CreatedUnit::decode(1, isize::MAX as usize, isize::MAX as usize),
        Some(CreatedUnit::Team { .. })
    ));
}

#[test]
fn explicit_start_geometry_rejects_bad_registers_and_legacy_stack_descriptors() {
    let end = 1usize << 38;
    let good = TaskStart {
        entry: VirtAddr::new(0x1002),
        stack_pointer: VirtAddr::new(end),
        arguments: VirtAddr::new(0x1ff8),
        argument_count: 2,
    };
    assert!(good.valid(end)); // Arguments span pages; SP is the exclusive bound.
    for sp in [0, 1, end - 1, end + 16, usize::MAX & !15] {
        assert!(
            !TaskStart {
                stack_pointer: VirtAddr::new(sp),
                ..good
            }
            .valid(end)
        );
    }
    for pc in [0, 1, end, usize::MAX] {
        assert!(
            !TaskStart {
                entry: VirtAddr::new(pc),
                ..good
            }
            .valid(end)
        );
    }
    for (address, count) in [
        (0, 1),
        (3, 1),
        (end - 8, 2),
        (usize::MAX - 7, 2),
        (0x1000, usize::MAX),
    ] {
        assert!(
            !TaskStart {
                arguments: VirtAddr::new(address),
                argument_count: count,
                ..good
            }
            .valid(end)
        );
    }
    assert!(
        TaskStart {
            arguments: VirtAddr::new(0),
            argument_count: 0,
            ..good
        }
        .valid(end)
    );
    let mut bytes = UnitCreate::Task {
        team: None,
        start: good,
    }
    .to_bytes();
    assert_eq!(&bytes[..8], &2usize.to_le_bytes());
    bytes[..8].copy_from_slice(&1usize.to_le_bytes());
    assert!(UnitCreate::from_bytes(&bytes).is_none());
}
