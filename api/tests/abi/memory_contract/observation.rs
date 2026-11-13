use super::request::{id, maintenance, set_word};
use super::{memo, slot};
use kernel_api::{
    abi::memory::{
        Action, Domain, Failure, Observation, Opcode, Outcome, Query, Reply, Request, Snapshot,
    },
    address::VirtAddr,
    model::{
        cell::{CellLevel, MappingId},
        flat::{
            BlockPhase, BlockStatus, FlatPhase, FlatStatus, FlatVersion, SlotPhase, SlotStatus,
            SlotVersion,
        },
        identity::{PendId, TaskId},
        nest::{
            MappingPhase, MappingStatus, MappingVersion, MemoGeneration, MemoPhase, MemoStatus,
            NestInfo, NestKind, NestPhase, NestStatus, NestVersion,
        },
    },
};

fn fixtures() -> Vec<(Query, Snapshot)> {
    let flat = slot(0).flat();
    let nest = memo(0).nest();
    let mapping = memo(0).mapping();
    vec![
        (
            Query::Flat {
                token: PendId::mint(7),
                flat,
            },
            Snapshot::Flat(FlatStatus {
                id: flat,
                version: FlatVersion::new(2).unwrap(),
                offset: 512,
                count: 1024,
                phase: FlatPhase::Open,
            }),
        ),
        (
            Query::Slot(slot(512)),
            Snapshot::Slot(SlotStatus {
                flat,
                coordinate: 512,
                version: SlotVersion::new(3).unwrap(),
                level: CellLevel::new(1).unwrap(),
                phase: SlotPhase::Resident,
                page: 53,
                obligations: 7,
                blocked: true,
            }),
        ),
        (
            Query::Block(maintenance()),
            Snapshot::Block(BlockStatus {
                block: maintenance().block,
                phase: BlockPhase::Held,
                offset: 512,
                count: 512,
                actor: TaskId::new(59),
                ordinary: 0,
                maintenance: 2,
                failed: false,
            }),
        ),
        (
            Query::Nest {
                token: PendId::mint(13),
                nest,
            },
            Snapshot::Nest(NestStatus {
                version: NestVersion::new(5).unwrap(),
                info: NestInfo {
                    id: nest,
                    kind: NestKind::External,
                    phase: NestPhase::Bound,
                    fault_sequence: u64::MAX,
                    fault_cause: 1 << 63,
                    fault_address: u64::MAX - 1,
                    fault_requester: u64::MAX,
                },
            }),
        ),
        (
            Query::Memo(memo(512)),
            Snapshot::Memo(MemoStatus {
                nest,
                mapping,
                coordinate: 512,
                generation: MemoGeneration::new(6).unwrap(),
                phase: MemoPhase::Published,
                source: Some((flat, 1024)),
                page: 53,
                access: 6,
                level: CellLevel::new(1).unwrap(),
            }),
        ),
        (
            Query::Mapping {
                token: PendId::mint(13),
                nest,
                mapping,
            },
            Snapshot::Mapping(MappingStatus {
                nest,
                mapping,
                version: MappingVersion::new(7).unwrap(),
                offset: 512,
                count: 512,
                address: VirtAddr::new(0x8000),
                level: CellLevel::new(1).unwrap(),
                access: 6,
                phase: MappingPhase::Open,
            }),
        ),
    ]
}

#[test]
fn inspect_queries_need_identity_and_scope_but_no_expected_version_or_request_id() {
    let expected: &[&[u64]] = &[
        &[11, 12, 18, 0, 0, 7, 11],
        &[11, 12, 18, 0, 1, 7, 11, 512],
        &[11, 12, 18, 0, 2, 23, 11, 29, 31],
        &[11, 13, 18, 0, 0, 13, 17],
        &[11, 13, 18, 0, 1, 13, 17, 19, 512],
        &[11, 13, 18, 0, 2, 13, 17, 19],
    ];
    for ((query, _), words) in fixtures().into_iter().zip(expected) {
        let request = Request::new(None, Action::Inspect(query)).unwrap();
        let mut bytes = [0; Request::WIDTH];
        for (i, word) in words.iter().enumerate() {
            set_word(&mut bytes, i, *word);
        }
        assert_eq!(request.to_bytes(), bytes);
        assert_eq!(
            Request::from_call(Opcode::Inspect(query.domain()).call_number(), &bytes),
            Some(request)
        );
        assert!(Request::new(Some(id()), Action::Inspect(query)).is_none());
        let mut bad = bytes;
        set_word(&mut bad, 3, 1);
        assert!(Request::from_bytes(&bad).is_none());
        let mut bad = bytes;
        set_word(&mut bad, 4, 6);
        assert!(Request::from_bytes(&bad).is_none());
        let mut bad = bytes;
        set_word(&mut bad, 5, 0);
        assert!(Request::from_bytes(&bad).is_none());
    }
    assert!(
        Request::new(
            None,
            Action::Inspect(Query::Flat {
                token: PendId::NONE,
                flat: slot(0).flat()
            })
        )
        .is_none()
    );
    assert!(
        Request::new(
            None,
            Action::Inspect(Query::Mapping {
                token: PendId::mint(1),
                nest: memo(0).nest(),
                mapping: MappingId::NONE
            })
        )
        .is_none()
    );
}

#[test]
fn observation_vectors_preserve_source_publication_and_full_width_faults() {
    let expected: &[&[u64]] = &[
        &[11, 12, 18, 0, 0, 0, 11, 2, 512, 1024, 0],
        &[11, 12, 18, 0, 1, 0, 11, 512, 3, 1, 1, 53, 7, 1],
        &[11, 12, 18, 0, 2, 0, 11, 29, 31, 1, 512, 512, 59, 0, 2, 0],
        &[
            11,
            13,
            18,
            0,
            0,
            0,
            17,
            5,
            1,
            1,
            u64::MAX,
            1 << 63,
            u64::MAX - 1,
            u64::MAX,
        ],
        &[11, 13, 18, 0, 1, 0, 17, 19, 512, 6, 3, 11, 1024, 53, 6, 1],
        &[11, 13, 18, 0, 2, 0, 17, 19, 7, 512, 512, 0x8000, 1, 6, 0],
    ];
    for ((query, snapshot), words) in fixtures().into_iter().zip(expected) {
        let observation = Observation::new(snapshot).unwrap();
        let mut bytes = [0; Observation::WIDTH];
        for (i, word) in words.iter().enumerate() {
            set_word(&mut bytes, i, *word);
        }
        assert_eq!(observation.to_bytes(), bytes);
        assert_eq!(Observation::from_bytes(&bytes), Some(observation));
        assert_eq!(observation.snapshot(), snapshot);
        assert!(observation.matches(Request::new(None, Action::Inspect(query)).unwrap()));
        for len in 0..bytes.len() {
            assert!(Observation::from_bytes(&bytes[..len]).is_none());
        }
        let mut longer = bytes.to_vec();
        longer.push(0);
        assert!(Observation::from_bytes(&longer).is_none());
    }
}

#[test]
fn observations_cannot_be_used_as_commit_receipts_or_for_another_position() {
    for (query, snapshot) in fixtures() {
        let observation = Observation::new(snapshot).unwrap();
        assert!(Reply::from_bytes(&observation.to_bytes()).is_none());
        let receipt = Reply::new(id(), Opcode::Occupy, Outcome::Rejected(Failure::Busy)).unwrap();
        assert!(Observation::from_bytes(&receipt.to_bytes()).is_none());
        assert!(!receipt.matches(Request::new(None, Action::Inspect(query)).unwrap()));
        assert!(
            !observation.matches(
                Request::new(
                    None,
                    Action::InspectResult {
                        domain: query.domain(),
                        request: id()
                    }
                )
                .unwrap()
            )
        );
    }
    let s = Observation::new(fixtures()[1].1).unwrap();
    assert!(!s.matches(Request::new(None, Action::Inspect(Query::Slot(slot(1024)))).unwrap()));
    let m = Observation::new(fixtures()[4].1).unwrap();
    assert!(!m.matches(Request::new(None, Action::Inspect(Query::Memo(memo(1024)))).unwrap()));
    assert!(!m.matches(Request::new(None, Action::Inspect(Query::Slot(slot(512)))).unwrap()));
}

#[test]
fn missing_slots_can_retain_history_but_not_a_current_holding() {
    let Snapshot::Slot(status) = fixtures()[1].1 else {
        unreachable!()
    };
    for phase in [SlotPhase::Fresh, SlotPhase::Missing, SlotPhase::Occupying] {
        assert!(Observation::new(Snapshot::Slot(SlotStatus { phase, ..status })).is_none());
        let no_holding = SlotStatus {
            phase,
            page: 0,
            obligations: 0,
            ..status
        };
        assert!(Observation::new(Snapshot::Slot(no_holding)).is_some());
    }
    for phase in [SlotPhase::Resident, SlotPhase::Vacating] {
        assert!(
            Observation::new(Snapshot::Slot(SlotStatus {
                phase,
                page: 0,
                ..status
            }))
            .is_none()
        );
    }
    let missing = SlotStatus {
        phase: SlotPhase::Missing,
        page: 0,
        obligations: 7,
        ..status
    };
    let observation = Observation::new(Snapshot::Slot(missing)).unwrap();
    assert_eq!(
        Observation::from_bytes(&observation.to_bytes())
            .unwrap()
            .snapshot(),
        Snapshot::Slot(missing)
    );
    assert!(
        Observation::new(Snapshot::Slot(SlotStatus {
            coordinate: 513,
            ..status
        }))
        .is_none()
    );
    assert!(
        Observation::new(Snapshot::Slot(SlotStatus {
            page: u64::MAX,
            ..status
        }))
        .is_none()
    );
}

#[test]
fn logical_association_can_exist_without_a_physical_publication() {
    let Snapshot::Memo(status) = fixtures()[4].1 else {
        unreachable!()
    };
    let associated = MemoStatus {
        phase: MemoPhase::Associated,
        page: 0,
        access: 0,
        ..status
    };
    assert!(Observation::new(Snapshot::Memo(associated)).is_some());
    let empty = MemoStatus {
        phase: MemoPhase::Empty,
        source: None,
        ..associated
    };
    assert!(Observation::new(Snapshot::Memo(empty)).is_some());
    for invalid in [
        MemoStatus {
            phase: MemoPhase::Empty,
            ..associated
        },
        MemoStatus {
            phase: MemoPhase::Associated,
            ..status
        },
        MemoStatus {
            source: None,
            ..status
        },
        MemoStatus { page: 0, ..status },
        MemoStatus {
            access: 4,
            ..status
        },
        MemoStatus {
            source: Some((slot(0).flat(), 1025)),
            ..status
        },
        MemoStatus {
            coordinate: 513,
            ..status
        },
    ] {
        assert!(Observation::new(Snapshot::Memo(invalid)).is_none());
    }
    for phase in [
        MemoPhase::Publishing,
        MemoPhase::Published,
        MemoPhase::Retiring,
        MemoPhase::Failed,
    ] {
        let observation = Observation::new(Snapshot::Memo(MemoStatus { phase, ..status })).unwrap();
        assert_eq!(
            Observation::from_bytes(&observation.to_bytes()),
            Some(observation)
        );
    }
}

#[test]
fn closed_maintenance_cannot_hide_unsettled_or_failed_responsibilities() {
    let Snapshot::Block(status) = fixtures()[2].1 else {
        unreachable!()
    };
    for invalid in [
        BlockStatus {
            phase: BlockPhase::Held,
            ordinary: 1,
            ..status
        },
        BlockStatus {
            phase: BlockPhase::Closed,
            ..status
        },
        BlockStatus {
            phase: BlockPhase::Closed,
            ordinary: 0,
            maintenance: 0,
            failed: true,
            ..status
        },
        BlockStatus { count: 0, ..status },
        BlockStatus {
            actor: TaskId::new(0),
            ..status
        },
    ] {
        assert!(Observation::new(Snapshot::Block(invalid)).is_none());
    }
    let closed = BlockStatus {
        phase: BlockPhase::Closed,
        ordinary: 0,
        maintenance: 0,
        failed: false,
        ..status
    };
    assert!(Observation::new(Snapshot::Block(closed)).is_some());
    let orphan = BlockStatus {
        phase: BlockPhase::Orphaned,
        ordinary: 3,
        maintenance: 2,
        failed: true,
        ..status
    };
    let observation = Observation::new(Snapshot::Block(orphan)).unwrap();
    assert_eq!(
        Observation::from_bytes(&observation.to_bytes()),
        Some(observation)
    );
}

#[test]
fn malformed_observations_reject_headers_geometry_flags_and_reserved_payloads() {
    for (_, snapshot) in fixtures() {
        let bytes = Observation::new(snapshot).unwrap().to_bytes();
        for (word, value) in [(0, 10), (1, 2), (2, 19), (3, 1), (4, 3), (5, 1), (6, 0)] {
            let mut bad = bytes;
            set_word(&mut bad, word, value);
            assert!(Observation::from_bytes(&bad).is_none());
        }
    }
    for (fixture, word, value) in [
        (0, 7, 0),
        (0, 9, 0),
        (0, 10, 4),
        (0, 11, 1),
        (1, 8, 0),
        (1, 9, 7),
        (1, 10, 7),
        (1, 13, 2),
        (1, 14, 1),
        (2, 7, 0),
        (2, 8, 0),
        (2, 9, 5),
        (2, 15, 2),
        (3, 7, 0),
        (3, 8, 2),
        (3, 9, 5),
        (3, 14, 1),
        (4, 7, 0),
        (4, 9, 0),
        (4, 10, 6),
        (4, 11, 0),
        (4, 15, 7),
        (5, 7, 0),
        (5, 8, 0),
        (5, 9, 513),
        (5, 10, 1),
        (5, 11, 1),
        (5, 12, 7),
        (5, 13, 4),
        (5, 14, 4),
        (5, 15, 1),
    ] {
        let mut bad = Observation::new(fixtures()[fixture].1).unwrap().to_bytes();
        set_word(&mut bad, word, value);
        assert!(
            Observation::from_bytes(&bad).is_none(),
            "fixture {fixture}, word {word}, value {value}"
        );
    }
    let Snapshot::Mapping(status) = fixtures()[5].1 else {
        unreachable!()
    };
    assert!(
        Observation::new(Snapshot::Mapping(MappingStatus {
            address: VirtAddr::new(usize::MAX & !4095),
            ..status
        }))
        .is_none()
    );
}

#[test]
fn accepted_query_and_observation_mutations_reencode_canonically() {
    for (query, snapshot) in fixtures() {
        let original = Request::new(None, Action::Inspect(query))
            .unwrap()
            .to_bytes();
        for i in 0..original.len() {
            let mut bytes = original;
            bytes[i] ^= 0xff;
            if let Some(request) = Request::from_bytes(&bytes) {
                assert_eq!(request.to_bytes(), bytes);
            }
        }
        let original = Observation::new(snapshot).unwrap().to_bytes();
        for i in 0..original.len() {
            let mut bytes = original;
            bytes[i] ^= 0xff;
            if let Some(observation) = Observation::from_bytes(&bytes) {
                assert_eq!(observation.to_bytes(), bytes);
            }
        }
    }
    assert_eq!(Opcode::Inspect(Domain::Flat).slot(), 18);
}
