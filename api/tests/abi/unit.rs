use kernel_api::{
    abi::register::Wire,
    address::VirtAddr,
    model::{
        identity::{TaskId, TeamId},
        unit::{
            ControlId, ExitCause, ProgramKind, UnitCreate, UnitInfo, UnitQuery, UnitState,
            UnitTarget,
        },
    },
};
#[test]
fn creation_variants_use_checked_fixed_width_snapshots() {
    for request in [
        UnitCreate::Team {
            nest: None,
            kind: ProgramKind::User,
            guard: ControlId::NONE,
        },
        UnitCreate::Task {
            team: Some(TeamId::new(9)),
            entry: VirtAddr::new(0x4000),
            args: VirtAddr::new(0x8000),
            count: 2,
            stack: VirtAddr::new(0),
        },
        UnitCreate::Team {
            nest: Some(kernel_api::model::identity::PendId::mint(9)),
            kind: ProgramKind::User,
            guard: ControlId::NONE,
        },
    ] {
        let bytes = request.to_bytes();
        assert_eq!(UnitCreate::from_bytes(&bytes), Some(request));
        assert!(UnitCreate::from_bytes(&bytes[..47]).is_none());
    }
    let mut team = UnitCreate::Team {
        nest: None,
        kind: ProgramKind::User,
        guard: ControlId::NONE,
    }
    .to_bytes();
    team[32] = 1;
    assert!(UnitCreate::from_bytes(&team).is_none());
    team[32] = 0;
    team[8] = 2;
    assert!(UnitCreate::from_bytes(&team).is_none());
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
