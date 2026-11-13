use super::{memo, slot};
use kernel_api::{
    abi::memory::{
        Action, Domain, Failure, Opcode, Outcome, RESULT_CAPACITY, Reply, Request, RequestId,
        Value,
        journal::{Accepted, Admission, Journal},
    },
    model::{flat::SlotVersion, identity::TaskId, nest::MemoGeneration},
};
fn actor() -> TaskId {
    TaskId::new(31)
}
fn request(id: u64) -> Request {
    Request::new(
        Some(RequestId::new(id).unwrap()),
        Action::Occupy {
            slot: slot(0),
            expected: SlotVersion::INITIAL,
        },
    )
    .unwrap()
}
fn execute(journal: &mut Journal, request: Request) -> Accepted {
    let Admission::Execute(accepted) = journal.begin(actor(), request).unwrap() else {
        panic!("new admission")
    };
    accepted
}
fn committed(id: u64) -> Reply {
    Reply::new(
        RequestId::new(id).unwrap(),
        Opcode::Occupy,
        Outcome::Committed(Value::Slot {
            version: SlotVersion::new(2).unwrap(),
            page: 17,
        }),
    )
    .unwrap()
}

#[test]
fn lost_delivery_replays_one_commit_without_reentering_execution() {
    let mut journal = Journal::new(actor()).unwrap();
    let accepted = execute(&mut journal, request(1));
    assert!(matches!(
        journal.begin(actor(), request(1)),
        Ok(Admission::InFlight(None))
    ));
    journal.record(&accepted, committed(1)).unwrap();
    // No output copy occurs here: the same retained result survives any number
    // of delivery failures. Only the first begin yielded Execute.
    for _ in 0..100 {
        assert!(
            matches!(journal.begin(actor(), request(1)), Ok(Admission::Replay(r)) if r == committed(1))
        );
    }
    assert_eq!(
        journal.inspect(actor(), Domain::Flat, RequestId::new(1).unwrap()),
        Ok(Some(committed(1)))
    );
    journal.record(&accepted, committed(1)).unwrap(); // identical completion is idempotent
    let rejected = Reply::new(
        RequestId::new(1).unwrap(),
        Opcode::Occupy,
        Outcome::Rejected(Failure::Busy),
    )
    .unwrap();
    assert_eq!(journal.record(&accepted, rejected), Err(Failure::Stale));
    assert_eq!(
        journal.inspect(actor(), Domain::Flat, RequestId::new(1).unwrap()),
        Ok(Some(committed(1)))
    );
}

#[test]
fn request_identity_covers_canonical_payload_and_domain_but_never_grants() {
    let mut journal = Journal::new(actor()).unwrap();
    let accepted = execute(&mut journal, request(1));
    for action in [
        Action::Occupy {
            slot: slot(1),
            expected: SlotVersion::INITIAL,
        },
        Action::Occupy {
            slot: slot(0),
            expected: SlotVersion::new(2).unwrap(),
        },
        Action::Remove {
            memo: memo(0),
            expected: MemoGeneration::INITIAL,
        },
    ] {
        let changed = Request::new(request(1).id(), action).unwrap();
        assert!(matches!(
            journal.begin(actor(), changed),
            Err(Failure::RequestMismatch)
        ));
    }
    assert!(matches!(
        journal.begin(TaskId::new(32), request(1)),
        Err(Failure::Denied)
    ));
    assert_eq!(
        journal.inspect(TaskId::new(32), Domain::Flat, request(1).id().unwrap()),
        Err(Failure::Denied)
    );
    assert_eq!(
        journal.inspect(actor(), Domain::Nest, request(1).id().unwrap()),
        Err(Failure::NoRegion)
    );
    assert_eq!(
        journal.record(&accepted, committed(2)),
        Err(Failure::Invalid)
    );
    assert!(Journal::new(TaskId::new(0)).is_none());
}

#[test]
fn all_inflight_slots_pin_capacity_and_busy_does_not_consume_a_sequence() {
    let mut journal = Journal::new(actor()).unwrap();
    let mut accepted = Vec::new();
    for id in 1..=RESULT_CAPACITY as u64 {
        accepted.push(execute(&mut journal, request(id)));
    }
    assert!(matches!(
        journal.begin(actor(), request(17)),
        Err(Failure::Busy)
    ));
    assert_eq!(journal.high_water(), 16);
    assert_eq!(
        journal.inspect(actor(), Domain::Flat, RequestId::new(17).unwrap()),
        Err(Failure::NoRegion)
    );
    journal.record(&accepted[0], committed(1)).unwrap();
    let _new = execute(&mut journal, request(17));
    assert_eq!(journal.high_water(), 17);
    assert!(matches!(
        journal.begin(actor(), request(1)),
        Err(Failure::ResultExpired)
    ));
    assert_eq!(
        journal.record(&accepted[0], committed(1)),
        Err(Failure::ResultExpired)
    );
    for id in 2..=16 {
        assert!(matches!(
            journal.begin(actor(), request(id)),
            Ok(Admission::InFlight(None))
        ));
    }
}

#[test]
fn pending_cannot_be_evicted_or_turned_into_no_effect_rejection() {
    let mut journal = Journal::new(actor()).unwrap();
    let req = Request::new(
        request(1).id(),
        Action::Publish {
            memo: memo(0),
            expected: MemoGeneration::INITIAL,
            maintenance: None,
        },
    )
    .unwrap();
    let accepted = execute(&mut journal, req);
    let value = Value::Memo {
        generation: MemoGeneration::new(2).unwrap(),
    };
    let pending = Reply::new(req.id().unwrap(), Opcode::Publish, Outcome::Pending(value)).unwrap();
    journal.record(&accepted, pending).unwrap();
    assert!(
        matches!(journal.begin(actor(), req), Ok(Admission::InFlight(Some(r))) if r == pending)
    );
    let rejected = Reply::new(
        req.id().unwrap(),
        Opcode::Publish,
        Outcome::Rejected(Failure::Busy),
    )
    .unwrap();
    assert_eq!(journal.record(&accepted, rejected), Err(Failure::Invalid));
    for id in 2..=40 {
        let ticket = execute(&mut journal, request(id));
        journal.record(&ticket, committed(id)).unwrap();
    }
    assert_eq!(
        journal.inspect(actor(), Domain::Nest, req.id().unwrap()),
        Ok(Some(pending))
    );
    let failed = Reply::new(
        req.id().unwrap(),
        Opcode::Publish,
        Outcome::Failed {
            failure: Failure::Hardware,
            retained: Some(value),
        },
    )
    .unwrap();
    journal.record(&accepted, failed).unwrap();
    assert!(matches!(journal.begin(actor(), req), Ok(Admission::Replay(r)) if r == failed));
    assert_eq!(journal.record(&accepted, pending), Err(Failure::Stale));
}

#[test]
fn gaps_eviction_and_exhaustion_never_make_an_old_identity_fresh() {
    let mut journal = Journal::new(actor()).unwrap();
    let accepted = execute(&mut journal, request(u64::MAX));
    journal.record(&accepted, committed(u64::MAX)).unwrap();
    for id in [1, 2, u64::MAX - 1] {
        assert!(matches!(
            journal.begin(actor(), request(id)),
            Err(Failure::ResultExpired)
        ));
        assert_eq!(
            journal.inspect(actor(), Domain::Flat, RequestId::new(id).unwrap()),
            Err(Failure::ResultExpired)
        );
    }
    assert!(matches!(
        journal.begin(actor(), request(u64::MAX)),
        Ok(Admission::Replay(_))
    ));
    assert_eq!(journal.high_water(), u64::MAX);
    assert!(
        core::mem::size_of::<Journal>() <= RESULT_CAPACITY * (Request::WIDTH + Reply::WIDTH) + 128
    );
}

#[test]
fn rejected_attempt_is_retained_but_inspections_and_expired_results_are_not_mutations() {
    let mut journal = Journal::new(actor()).unwrap();
    let req = request(1);
    let accepted = execute(&mut journal, req);
    let expired = Reply::new(req.id().unwrap(), Opcode::Occupy, Outcome::Expired).unwrap();
    assert_eq!(journal.record(&accepted, expired), Err(Failure::Invalid));
    let rejected = Reply::new(
        req.id().unwrap(),
        Opcode::Occupy,
        Outcome::Rejected(Failure::Denied),
    )
    .unwrap();
    journal.record(&accepted, rejected).unwrap();
    assert!(matches!(journal.begin(actor(), req), Ok(Admission::Replay(r)) if r == rejected));
    let query = Request::new(
        None,
        Action::InspectResult {
            domain: Domain::Flat,
            request: req.id().unwrap(),
        },
    )
    .unwrap();
    assert!(matches!(
        journal.begin(actor(), query),
        Err(Failure::Invalid)
    ));
    assert_eq!(journal.high_water(), 1);
}

#[test]
fn takeover_updates_retained_epoch_before_a_later_failure() {
    use kernel_api::model::flat::{BlockPhase, BlockRef};
    let old = super::request::maintenance().block;
    let next = BlockRef::new(old.flat(), old.record(), old.generation().next().unwrap()).unwrap();
    let req = Request::new(
        request(1).id(),
        Action::Block {
            start: slot(0),
            count: 1,
            recover: Some(old),
        },
    )
    .unwrap();
    let mut journal = Journal::new(actor()).unwrap();
    let accepted = execute(&mut journal, req);
    for block in [old, next] {
        journal
            .record(
                &accepted,
                Reply::new(
                    req.id().unwrap(),
                    Opcode::Block,
                    Outcome::Pending(Value::Maintenance {
                        block,
                        phase: BlockPhase::Draining,
                    }),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let failed = Reply::new(
        req.id().unwrap(),
        Opcode::Block,
        Outcome::Failed {
            failure: Failure::Busy,
            retained: Some(Value::Maintenance {
                block: next,
                phase: BlockPhase::Draining,
            }),
        },
    )
    .unwrap();
    journal.record(&accepted, failed).unwrap();
    assert_eq!(
        journal.inspect(actor(), Domain::Flat, req.id().unwrap()),
        Ok(Some(failed))
    );
    assert!(matches!(journal.begin(actor(), req), Ok(Admission::Replay(reply)) if reply == failed));
}
