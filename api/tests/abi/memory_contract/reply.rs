use super::request::{id, maintenance, set_word};
use super::{memo, slot};
use kernel_api::{
    abi::memory::{
        Action, Delivery, Domain, Failure, Opcode, Outcome, Reply, Request, RequestId, Value,
    },
    model::{
        flat::{BlockPhase, SlotVersion},
        nest::MemoGeneration,
    },
};

fn slot_value(page: u64) -> Value {
    Value::Slot {
        version: SlotVersion::new(1 << 63).unwrap(),
        page,
    }
}
fn memo_value() -> Value {
    Value::Memo {
        generation: MemoGeneration::new(u64::MAX).unwrap(),
    }
}
fn block_value(phase: BlockPhase) -> Value {
    Value::Maintenance {
        block: maintenance().block,
        phase,
    }
}

fn results() -> Vec<Reply> {
    let mut cases = vec![
        (Opcode::Occupy, Outcome::Committed(slot_value(17))),
        (Opcode::Vacate, Outcome::Pending(slot_value(17))),
        (Opcode::Vacate, Outcome::Committed(slot_value(0))),
        (
            Opcode::Block,
            Outcome::Pending(block_value(BlockPhase::Draining)),
        ),
        (
            Opcode::Block,
            Outcome::Committed(block_value(BlockPhase::Held)),
        ),
        (
            Opcode::Allow,
            Outcome::Pending(block_value(BlockPhase::Releasing)),
        ),
        (
            Opcode::Allow,
            Outcome::Committed(block_value(BlockPhase::Closed)),
        ),
        (
            Opcode::Block,
            Outcome::Failed {
                failure: Failure::Dead,
                retained: Some(block_value(BlockPhase::Orphaned)),
            },
        ),
        (Opcode::Inspect(Domain::Flat), Outcome::Expired),
        (Opcode::Inspect(Domain::Nest), Outcome::Expired),
    ];
    for opcode in [
        Opcode::Insert,
        Opcode::Remove,
        Opcode::Publish,
        Opcode::Retract,
        Opcode::Protect,
    ] {
        cases.push((opcode, Outcome::Committed(memo_value())));
    }
    for opcode in [Opcode::Publish, Opcode::Retract, Opcode::Protect] {
        cases.push((opcode, Outcome::Pending(memo_value())));
        cases.push((
            opcode,
            Outcome::Failed {
                failure: Failure::Hardware,
                retained: Some(memo_value()),
            },
        ));
    }
    for opcode in [Opcode::Push, Opcode::Pull] {
        cases.push((
            opcode,
            Outcome::Committed(Value::Transfer {
                local: SlotVersion::INITIAL,
                remote: SlotVersion::new(2).unwrap(),
                page: 17,
            }),
        ));
    }
    for failure in [
        Failure::Denied,
        Failure::Dead,
        Failure::Busy,
        Failure::OutOfMemory,
        Failure::NotAligned,
        Failure::NoRegion,
        Failure::Stale,
        Failure::SourceEmpty,
        Failure::DestinationOccupied,
        Failure::NotTransferable,
        Failure::Unsupported,
        Failure::VersionExhausted,
        Failure::RequestMismatch,
        Failure::Invalid,
        Failure::Hardware,
    ] {
        cases.push((Opcode::Occupy, Outcome::Rejected(failure)));
    }
    cases.push((
        Opcode::Occupy,
        Outcome::Failed {
            failure: Failure::OutOfMemory,
            retained: None,
        },
    ));
    let mut replies: Vec<_> = cases
        .into_iter()
        .map(|(opcode, outcome)| Reply::new(id(), opcode, outcome).unwrap())
        .collect();
    replies.extend(super::objects::results());
    replies.extend(super::reshape::results());
    replies
}

#[test]
fn failures_have_explicit_codes_and_unknown_codes_reject() {
    let failures = [
        Failure::Denied,
        Failure::Dead,
        Failure::Busy,
        Failure::OutOfMemory,
        Failure::NotAligned,
        Failure::NoRegion,
        Failure::Stale,
        Failure::SourceEmpty,
        Failure::DestinationOccupied,
        Failure::NotTransferable,
        Failure::Unsupported,
        Failure::VersionExhausted,
        Failure::RequestMismatch,
        Failure::ResultExpired,
        Failure::ReplyFault,
        Failure::Invalid,
        Failure::Hardware,
    ];
    for (index, failure) in failures.into_iter().enumerate() {
        assert_eq!(failure.code(), -(index as i64) - 1);
        assert_eq!(Failure::from_code(-(index as i64) - 1), Some(failure));
    }
    for code in [i64::MIN, -18, 0, 1, i64::MAX] {
        assert!(Failure::from_code(code).is_none());
    }
}

#[test]
fn committed_and_rejected_results_have_independent_byte_vectors() {
    let committed = Reply::new(id(), Opcode::Occupy, Outcome::Committed(slot_value(17))).unwrap();
    let mut expected = [0u8; 128];
    for (index, word) in [11, 12, 19, u64::MAX, 2, 0, 1, 1 << 63, 17]
        .into_iter()
        .enumerate()
    {
        set_word(&mut expected, index, word);
    }
    assert_eq!(committed.to_bytes(), expected);
    assert_eq!(Reply::from_bytes(&expected), Some(committed));
    expected = [0; 128];
    for (index, word) in [11, 12, 19, u64::MAX, 0, (-3i64) as u64, 0]
        .into_iter()
        .enumerate()
    {
        set_word(&mut expected, index, word);
    }
    let rejected = Reply::new(id(), Opcode::Occupy, Outcome::Rejected(Failure::Busy)).unwrap();
    assert_eq!(rejected.to_bytes(), expected);
    assert_eq!(Reply::from_bytes(&expected), Some(rejected));
    for result in results() {
        assert_eq!(Reply::from_bytes(&result.to_bytes()), Some(result));
    }
}

#[test]
fn pending_and_failure_cannot_masquerade_as_committed_or_no_effect() {
    for (opcode, outcome) in [
        (Opcode::Occupy, Outcome::Pending(slot_value(17))),
        (Opcode::Occupy, Outcome::Committed(slot_value(0))),
        (Opcode::Vacate, Outcome::Committed(slot_value(17))),
        (Opcode::Vacate, Outcome::Pending(slot_value(0))),
        (
            Opcode::Block,
            Outcome::Committed(block_value(BlockPhase::Draining)),
        ),
        (
            Opcode::Block,
            Outcome::Pending(block_value(BlockPhase::Held)),
        ),
        (
            Opcode::Allow,
            Outcome::Committed(block_value(BlockPhase::Releasing)),
        ),
        (
            Opcode::Allow,
            Outcome::Pending(block_value(BlockPhase::Closed)),
        ),
        (Opcode::Insert, Outcome::Pending(memo_value())),
        (Opcode::Remove, Outcome::Pending(memo_value())),
        (Opcode::Occupy, Outcome::Committed(memo_value())),
        (Opcode::Publish, Outcome::Committed(slot_value(17))),
        (
            Opcode::Inspect(Domain::Flat),
            Outcome::Committed(slot_value(17)),
        ),
        (Opcode::Occupy, Outcome::Rejected(Failure::ResultExpired)),
        (Opcode::Occupy, Outcome::Rejected(Failure::ReplyFault)),
        (
            Opcode::Occupy,
            Outcome::Failed {
                failure: Failure::RequestMismatch,
                retained: None,
            },
        ),
        (
            Opcode::Occupy,
            Outcome::Failed {
                failure: Failure::ReplyFault,
                retained: None,
            },
        ),
        (Opcode::Occupy, Outcome::Committed(slot_value(u64::MAX))),
        (
            Opcode::Push,
            Outcome::Committed(Value::Transfer {
                local: SlotVersion::INITIAL,
                remote: SlotVersion::INITIAL,
                page: 0,
            }),
        ),
    ] {
        assert!(
            Reply::new(id(), opcode, outcome).is_none(),
            "{opcode:?}: {outcome:?}"
        );
    }
    let pending = Reply::new(
        id(),
        Opcode::Block,
        Outcome::Pending(block_value(BlockPhase::Draining)),
    )
    .unwrap();
    let mut bytes = pending.to_bytes();
    set_word(&mut bytes, 4, 2); // Claiming commit while still Draining is malformed.
    assert!(Reply::from_bytes(&bytes).is_none());
    let mut bytes = pending.to_bytes();
    set_word(&mut bytes, 4, 0);
    set_word(&mut bytes, 5, (-3i64) as u64);
    assert!(Reply::from_bytes(&bytes).is_none()); // Rejection cannot retain a Block payload.
}

#[test]
fn replies_correlate_to_operation_identity_and_result_inspection() {
    let request = Request::new(
        Some(id()),
        Action::Occupy {
            slot: slot(512),
            expected: SlotVersion::INITIAL,
        },
    )
    .unwrap();
    let reply = Reply::new(id(), Opcode::Occupy, Outcome::Committed(slot_value(17))).unwrap();
    assert!(reply.matches(request));
    assert_eq!(reply.request(), id());
    assert_eq!(reply.opcode(), Opcode::Occupy);
    assert_eq!(reply.outcome(), Outcome::Committed(slot_value(17)));
    let old = RequestId::new(1).unwrap();
    assert!(
        !Reply::new(old, Opcode::Occupy, reply.outcome())
            .unwrap()
            .matches(request)
    );
    let wrong = Request::new(
        Some(id()),
        Action::Publish {
            memo: memo(512),
            expected: MemoGeneration::INITIAL,
            maintenance: None,
        },
    )
    .unwrap();
    assert!(!reply.matches(wrong));
    let inspect = Request::new(
        None,
        Action::InspectResult {
            domain: Domain::Flat,
            request: id(),
        },
    )
    .unwrap();
    assert!(reply.matches(inspect));
    let expired = Reply::new(id(), Opcode::Inspect(Domain::Flat), Outcome::Expired).unwrap();
    assert!(expired.matches(inspect));
    let other_domain = Request::new(
        None,
        Action::InspectResult {
            domain: Domain::Nest,
            request: id(),
        },
    )
    .unwrap();
    assert!(!expired.matches(other_domain));
    let other_request = Request::new(
        None,
        Action::InspectResult {
            domain: Domain::Flat,
            request: old,
        },
    )
    .unwrap();
    assert!(!expired.matches(other_request));
}

#[test]
fn malformed_reply_tags_statuses_and_tail_words_reject() {
    let good = Reply::new(id(), Opcode::Occupy, Outcome::Committed(slot_value(17)))
        .unwrap()
        .to_bytes();
    for (word, value) in [
        (0, 10),
        (1, 2),
        (2, 0),
        (3, 0),
        (4, 5),
        (5, 1),
        (5, (-17i64) as u64),
        (6, 0),
        (6, 12),
        (7, 0),
        (8, 0),
        (9, 1),
        (15, 1),
    ] {
        let mut bad = good;
        set_word(&mut bad, word, value);
        assert!(Reply::from_bytes(&bad).is_none(), "word {word}, {value}");
    }
    for len in 0..Reply::WIDTH {
        assert!(Reply::from_bytes(&good[..len]).is_none());
    }
    let mut bad = good.to_vec();
    bad.push(0);
    assert!(Reply::from_bytes(&bad).is_none());
    let expired = Reply::new(id(), Opcode::Inspect(Domain::Flat), Outcome::Expired).unwrap();
    let mut bad = expired.to_bytes();
    set_word(&mut bad, 5, 0);
    assert!(Reply::from_bytes(&bad).is_none());
    let mut bad = Reply::new(
        id(),
        Opcode::Block,
        Outcome::Committed(block_value(BlockPhase::Held)),
    )
    .unwrap()
    .to_bytes();
    set_word(&mut bad, 10, 5);
    assert!(Reply::from_bytes(&bad).is_none());
}

#[test]
fn accepted_reply_bytes_have_no_noncanonical_state_combinations() {
    for result in results() {
        let original = result.to_bytes();
        for index in 0..original.len() {
            let mut bytes = original;
            bytes[index] ^= 0xff;
            if let Some(decoded) = Reply::from_bytes(&bytes) {
                assert_eq!(
                    decoded.to_bytes(),
                    bytes,
                    "noncanonical byte {index} in {result:?}"
                );
            }
        }
    }
}

#[test]
fn register_delivery_is_not_the_stored_mutation_outcome() {
    assert_eq!(Delivery::Written.registers(), [0, 128, 0]);
    assert_eq!(
        Delivery::from_registers([0, 128, 0]),
        Some(Delivery::Written)
    );
    for code in -17..=-1 {
        let status = Delivery::Failed(Failure::from_code(code).unwrap());
        assert_eq!(status.registers(), [code as u64, 0, 0]);
        assert_eq!(Delivery::from_registers(status.registers()), Some(status));
    }
    for words in [
        [0, 0, 0],
        [0, 127, 0],
        [0, 128, 1],
        [1, 0, 0],
        [(-18i64) as u64, 0, 0],
        [(-15i64) as u64, 128, 0],
    ] {
        assert!(Delivery::from_registers(words).is_none());
    }
    // Delivery can fail after commit; the retained record remains committed.
    let reply = Reply::new(id(), Opcode::Occupy, Outcome::Committed(slot_value(17))).unwrap();
    let bytes = reply.to_bytes();
    assert_eq!(
        Delivery::from_registers([(-15i64) as u64, 0, 0]),
        Some(Delivery::Failed(Failure::ReplyFault))
    );
    assert_eq!(
        Reply::from_bytes(&bytes).unwrap().outcome(),
        Outcome::Committed(slot_value(17))
    );
}

#[test]
fn failed_outcomes_do_not_claim_atomic_commit_or_discard_known_maintenance() {
    for (opcode, retained) in [
        (Opcode::Occupy, Some(slot_value(17))),
        (Opcode::Occupy, Some(slot_value(0))),
        (Opcode::Insert, Some(memo_value())),
        (Opcode::Vacate, Some(slot_value(0))),
        (Opcode::Vacate, None),
        (Opcode::Block, Some(block_value(BlockPhase::Held))),
        (Opcode::Block, Some(block_value(BlockPhase::Closed))),
        (Opcode::Block, None),
        (Opcode::Allow, Some(block_value(BlockPhase::Closed))),
        (Opcode::Allow, None),
    ] {
        assert!(
            Reply::new(
                id(),
                opcode,
                Outcome::Failed {
                    failure: Failure::Hardware,
                    retained
                }
            )
            .is_none()
        );
    }
    for (opcode, retained) in [
        (Opcode::Vacate, Some(slot_value(17))),
        (Opcode::Publish, Some(memo_value())),
        (Opcode::Block, Some(block_value(BlockPhase::Draining))),
        (Opcode::Allow, Some(block_value(BlockPhase::Releasing))),
        (Opcode::Block, Some(block_value(BlockPhase::Orphaned))),
        (Opcode::Allow, Some(block_value(BlockPhase::Orphaned))),
    ] {
        let result = Reply::new(
            id(),
            opcode,
            Outcome::Failed {
                failure: Failure::Hardware,
                retained,
            },
        )
        .unwrap();
        assert_eq!(Reply::from_bytes(&result.to_bytes()), Some(result));
    }
}
