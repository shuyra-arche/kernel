use super::request::{id, set_word};
use super::{memo, slot};
use kernel_api::{
    abi::memory::{
        Action, Delivery, Failure, Observation, Opcode, Outcome, Query, Reply, Request, Snapshot,
        Value,
    },
    model::{
        flat::SlotVersion,
        identity::{PendId, TaskId},
        nest::{
            FAULT_CAPACITY, FaultAccess, FaultCause, FaultEpoch, FaultEvent, FaultPhase,
            FaultRoute, FaultSequence, FaultStatus, MAX_FAULT_NS, MemoGeneration,
            ReservationVersion, fault_read_deadline,
        },
        pend::NestRights,
        wait::Wait,
    },
};

fn route() -> FaultRoute {
    FaultRoute {
        sign: PendId::mint(23),
        timeout_ns: MAX_FAULT_NS,
    }
}

pub(super) fn mutations() -> Vec<Action> {
    [None, Some(route())]
        .into_iter()
        .map(|route| Action::FaultRoute {
            token: PendId::mint(13),
            nest: memo(0).nest(),
            reservation: memo(0).reservation(),
            expected: ReservationVersion::new(7).unwrap(),
            epoch: FaultEpoch::new(3).unwrap(),
            route,
        })
        .collect()
}

pub(super) fn results() -> Vec<Reply> {
    [FaultPhase::Disabled, FaultPhase::Active]
        .into_iter()
        .map(|phase| {
            Reply::new(
                id(),
                Opcode::FaultRoute,
                Outcome::Committed(Value::FaultRoute {
                    nest: memo(0).nest(),
                    reservation: memo(0).reservation(),
                    version: ReservationVersion::new(8).unwrap(),
                    epoch: FaultEpoch::new(4).unwrap(),
                    phase,
                }),
            )
            .unwrap()
        })
        .collect()
}

fn event() -> FaultEvent {
    FaultEvent {
        nest: memo(0).nest(),
        reservation: memo(0).reservation(),
        epoch: FaultEpoch::new(4).unwrap(),
        sequence: FaultSequence::new(u64::MAX).unwrap(),
        memo_page: 512,
        generation: MemoGeneration::new(1 << 63).unwrap(),
        flat: slot(0).flat(),
        slot_page: 1024,
        version: SlotVersion::new(u64::MAX).unwrap(),
        cause: FaultCause::Missing,
        access: FaultAccess::Write,
    }
}

fn status() -> FaultStatus {
    FaultStatus {
        nest: memo(0).nest(),
        reservation: memo(0).reservation(),
        epoch: FaultEpoch::new(4).unwrap(),
        phase: FaultPhase::Active,
        handler: Some(TaskId::new(29)),
        timeout_ns: MAX_FAULT_NS,
        last_sequence: u64::MAX,
    }
}

fn queries() -> Vec<Query> {
    vec![
        Query::FaultRoute {
            token: PendId::mint(13),
            nest: memo(0).nest(),
            reservation: memo(0).reservation(),
        },
        Query::FaultEvent {
            token: PendId::mint(13),
            nest: memo(0).nest(),
            reservation: memo(0).reservation(),
            epoch: FaultEpoch::new(4).unwrap(),
            after: u64::MAX - 1,
            wait: Wait::At(1 << 63),
        },
    ]
}

fn bytes<const N: usize>(words: &[u64]) -> [u8; N] {
    let mut bytes = [0; N];
    for (i, word) in words.iter().enumerate() {
        set_word(&mut bytes, i, *word);
    }
    bytes
}

#[test]
fn route_vectors_have_no_caller_selected_handler_or_authority_root() {
    assert_eq!(NestRights::from_bits(1 << 12), Some(NestRights::FAULT));
    for (index, (action, reply)) in mutations().into_iter().zip(results()).enumerate() {
        let request = Request::new(Some(id()), action).unwrap();
        let mut expected = bytes::<256>(&[11, 13, 26, u64::MAX, 13, 17, 19, 7, 3, index as u64]);
        if index == 1 {
            set_word(&mut expected, 10, 23);
            set_word(&mut expected, 11, MAX_FAULT_NS);
        }
        assert_eq!(request.to_bytes(), expected);
        assert_eq!(
            Request::from_call(Opcode::FaultRoute.call_number(), &expected),
            Some(request)
        );
        assert!(Request::new(None, action).is_none());
        let expected = bytes::<128>(&[11, 13, 26, u64::MAX, 2, 0, 12, 17, 19, 8, 4, index as u64]);
        assert_eq!(reply.to_bytes(), expected);
        assert_eq!(Reply::from_bytes(&expected), Some(reply));
        assert!(reply.matches(request));
        let mut bad = request.to_bytes();
        set_word(&mut bad, 12, 29); // cannot smuggle a handler identity into the zero tail
        assert!(Request::from_bytes(&bad).is_none());
    }
}

#[test]
fn fault_observation_vectors_preserve_source_and_full_width_event_identity() {
    let expected_queries = [
        bytes::<256>(&[11, 13, 18, 0, 4, 13, 17, 19]),
        bytes::<256>(&[11, 13, 18, 0, 5, 13, 17, 19, 4, u64::MAX - 1, 1, 1 << 63]),
    ];
    let snapshots = [
        Snapshot::FaultRoute(status()),
        Snapshot::FaultEvent(event()),
    ];
    let expected_snapshots = [
        bytes::<128>(&[
            11,
            13,
            18,
            0,
            4,
            0,
            17,
            19,
            4,
            1,
            29,
            MAX_FAULT_NS,
            u64::MAX,
        ]),
        bytes::<128>(&[
            11,
            13,
            18,
            0,
            5,
            0,
            17,
            19,
            4,
            u64::MAX,
            512,
            1 << 63,
            11,
            1024,
            u64::MAX,
            0x401,
        ]),
    ];
    for (index, query) in queries().into_iter().enumerate() {
        let request = Request::new(None, Action::Inspect(query)).unwrap();
        let observation = Observation::new(snapshots[index]).unwrap();
        assert_eq!(request.to_bytes(), expected_queries[index]);
        assert_eq!(
            Request::from_call(
                Opcode::Inspect(kernel_api::abi::memory::Domain::Nest).call_number(),
                &expected_queries[index]
            ),
            Some(request)
        );
        assert_eq!(observation.to_bytes(), expected_snapshots[index]);
        assert_eq!(
            Observation::from_bytes(&expected_snapshots[index]),
            Some(observation)
        );
        assert!(observation.matches(request));
        assert!(Reply::from_bytes(&expected_snapshots[index]).is_none());
        for len in 0..128 {
            assert!(Observation::from_bytes(&expected_snapshots[index][..len]).is_none());
        }
        assert!(Observation::from_bytes(&[0; 129]).is_none());
    }
    for cause in [FaultCause::Fresh, FaultCause::Missing] {
        for access in [FaultAccess::Read, FaultAccess::Write, FaultAccess::Execute] {
            let observation = Observation::new(Snapshot::FaultEvent(FaultEvent {
                cause,
                access,
                ..event()
            }))
            .unwrap();
            assert_eq!(
                Observation::from_bytes(&observation.to_bytes()),
                Some(observation)
            );
        }
    }
}

#[test]
fn recovery_budgets_and_read_deadlines_are_finite_and_overflow_checked() {
    assert_eq!(FAULT_CAPACITY, 16);
    assert_eq!(route().deadline(100), Some(100 + MAX_FAULT_NS));
    assert!(route().deadline(u64::MAX - MAX_FAULT_NS + 1).is_none());
    for timeout_ns in [0, MAX_FAULT_NS + 1, u64::MAX] {
        let invalid = FaultRoute {
            timeout_ns,
            ..route()
        };
        assert!(!invalid.valid());
        assert!(invalid.deadline(0).is_none());
    }
    assert_eq!(fault_read_deadline(Wait::Poll, 100), Some(100));
    assert_eq!(fault_read_deadline(Wait::At(99), 100), Some(99));
    assert_eq!(
        fault_read_deadline(Wait::At(100 + MAX_FAULT_NS), 100),
        Some(100 + MAX_FAULT_NS)
    );
    assert!(fault_read_deadline(Wait::At(101 + MAX_FAULT_NS), 100).is_none());
    assert!(fault_read_deadline(Wait::Infinite, 100).is_none());
    let Query::FaultEvent {
        token,
        nest,
        reservation,
        epoch,
        after,
        ..
    } = queries()[1]
    else {
        unreachable!()
    };
    assert!(
        Request::new(
            None,
            Action::Inspect(Query::FaultEvent {
                token,
                nest,
                reservation,
                epoch,
                after,
                wait: Wait::Infinite
            })
        )
        .is_none()
    );
    let poll = Request::new(
        None,
        Action::Inspect(Query::FaultEvent {
            token,
            nest,
            reservation,
            epoch,
            after,
            wait: Wait::Poll,
        }),
    )
    .unwrap();
    assert_eq!(Request::from_bytes(&poll.to_bytes()), Some(poll));
    let mut bad = poll.to_bytes();
    set_word(&mut bad, 11, 1);
    assert!(Request::from_bytes(&bad).is_none());
}

#[test]
fn stale_route_events_and_mismatched_registration_receipts_do_not_correlate() {
    let request = Request::new(None, Action::Inspect(queries()[1])).unwrap();
    for bad in [
        FaultEvent {
            epoch: FaultEpoch::new(3).unwrap(),
            ..event()
        },
        FaultEvent {
            sequence: FaultSequence::new(u64::MAX - 1).unwrap(),
            ..event()
        },
    ] {
        assert!(
            !Observation::new(Snapshot::FaultEvent(bad))
                .unwrap()
                .matches(request)
        );
    }
    for (action, reply) in mutations().into_iter().zip(results()) {
        let request = Request::new(Some(id()), action).unwrap();
        for (index, value) in [(7, 37), (8, 41), (9, 9), (10, 5)] {
            let mut bad = reply.to_bytes();
            set_word(&mut bad, index, value);
            assert!(!Reply::from_bytes(&bad).unwrap().matches(request));
        }
        let mut bad = reply.to_bytes();
        set_word(
            &mut bad,
            11,
            if matches!(action, Action::FaultRoute { route: None, .. }) {
                1
            } else {
                0
            },
        );
        assert!(!Reply::from_bytes(&bad).unwrap().matches(request));
    }
    assert!(FaultEpoch::new(0).is_none());
    assert!(FaultSequence::new(0).is_none());
    assert!(FaultEpoch::new(u64::MAX).unwrap().next().is_none());
    assert!(FaultSequence::new(u64::MAX).unwrap().next().is_none());
    assert_eq!(FaultEpoch::INITIAL.next().unwrap().get(), 2);
    assert_eq!(
        FaultSequence::new(1 << 63).unwrap().next().unwrap().get(),
        (1 << 63) + 1
    );
}

#[test]
fn route_loss_and_disabled_status_cannot_masquerade_as_active_service() {
    let lost = Observation::new(Snapshot::FaultRoute(FaultStatus {
        phase: FaultPhase::Lost,
        ..status()
    }))
    .unwrap();
    assert_eq!(Observation::from_bytes(&lost.to_bytes()), Some(lost));
    let disabled = FaultStatus {
        phase: FaultPhase::Disabled,
        handler: None,
        timeout_ns: 0,
        ..status()
    };
    assert!(Observation::new(Snapshot::FaultRoute(disabled)).is_some());
    assert!(
        Observation::new(Snapshot::FaultRoute(FaultStatus {
            epoch: FaultEpoch::INITIAL,
            ..disabled
        }))
        .is_none()
    );
    let initial = FaultStatus {
        epoch: FaultEpoch::INITIAL,
        last_sequence: 0,
        ..disabled
    };
    assert!(Observation::new(Snapshot::FaultRoute(initial)).is_some());
    for invalid in [
        FaultStatus {
            handler: None,
            ..status()
        },
        FaultStatus {
            timeout_ns: 0,
            ..status()
        },
        FaultStatus {
            phase: FaultPhase::Disabled,
            ..status()
        },
        FaultStatus {
            handler: Some(TaskId::new(0)),
            ..status()
        },
    ] {
        assert!(Observation::new(Snapshot::FaultRoute(invalid)).is_none());
    }
    for reply in results() {
        let Outcome::Committed(value) = reply.outcome() else {
            unreachable!()
        };
        assert!(Reply::new(id(), Opcode::FaultRoute, Outcome::Pending(value)).is_none());
        assert!(
            Reply::new(
                id(),
                Opcode::FaultRoute,
                Outcome::Failed {
                    failure: Failure::HandlerGone,
                    retained: Some(value)
                }
            )
            .is_none()
        );
        let mut bad = reply.to_bytes();
        set_word(&mut bad, 11, 2);
        assert!(Reply::from_bytes(&bad).is_none());
    }
    assert!(
        Reply::new(
            id(),
            Opcode::FaultRoute,
            Outcome::Rejected(Failure::FaultExpired)
        )
        .is_none()
    );
    assert_eq!(
        Delivery::from_registers(Delivery::Failed(Failure::FaultExpired).registers()),
        Some(Delivery::Failed(Failure::FaultExpired))
    );
}

#[test]
fn malformed_fault_fields_reject_unknown_flags_and_noncanonical_tails() {
    let request = Request::new(Some(id()), mutations()[1]).unwrap().to_bytes();
    for (i, value) in [
        (4, 0),
        (5, 0),
        (6, 0),
        (7, 0),
        (8, 0),
        (9, 2),
        (10, 0),
        (11, 0),
        (11, MAX_FAULT_NS + 1),
        (12, 1),
    ] {
        let mut bad = request;
        set_word(&mut bad, i, value);
        assert!(Request::from_bytes(&bad).is_none(), "request {i}");
    }
    let query = Request::new(None, Action::Inspect(queries()[1]))
        .unwrap()
        .to_bytes();
    for (i, value) in [
        (3, 1),
        (4, 6),
        (5, 0),
        (6, 0),
        (7, 0),
        (8, 0),
        (10, 2),
        (12, 1),
    ] {
        let mut bad = query;
        set_word(&mut bad, i, value);
        assert!(Request::from_bytes(&bad).is_none(), "query {i}");
    }
    let original = Observation::new(Snapshot::FaultEvent(event()))
        .unwrap()
        .to_bytes();
    for (i, value) in [
        (0, 10),
        (1, 12),
        (2, 26),
        (3, 1),
        (4, 6),
        (5, 1),
        (6, 0),
        (7, 0),
        (8, 0),
        (8, 1),
        (9, 0),
        (10, u64::MAX),
        (11, 0),
        (12, 0),
        (13, u64::MAX),
        (14, 0),
        (15, 0),
        (15, 0x402),
        (15, 0x601),
        (15, 0x10401),
    ] {
        let mut bad = original;
        set_word(&mut bad, i, value);
        assert!(
            Observation::from_bytes(&bad).is_none(),
            "event {i}, {value}"
        );
    }
    let original = Observation::new(Snapshot::FaultRoute(status()))
        .unwrap()
        .to_bytes();
    for (i, value) in [
        (7, 0),
        (8, 0),
        (9, 3),
        (10, 0),
        (11, 0),
        (11, MAX_FAULT_NS + 1),
        (13, 1),
    ] {
        let mut bad = original;
        set_word(&mut bad, i, value);
        assert!(Observation::from_bytes(&bad).is_none(), "status {i}");
    }
}

#[test]
fn accepted_fault_observations_and_queries_have_one_canonical_encoding() {
    for query in queries() {
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
    }
    for snapshot in [
        Snapshot::FaultRoute(status()),
        Snapshot::FaultEvent(event()),
    ] {
        let original = Observation::new(snapshot).unwrap().to_bytes();
        for i in 0..original.len() {
            let mut bytes = original;
            bytes[i] ^= 0xff;
            if let Some(observation) = Observation::from_bytes(&bytes) {
                assert_eq!(observation.to_bytes(), bytes);
            }
        }
    }
}
