use super::worker::Worker;
use super::*;
use core::fmt::Write;
use kernel_api::model::{cell::*, flat::*};

pub fn run() {
    let whole = space();
    let (a, _) = reserve_pages(whole, Placement::lowest(4096).unwrap(), 6, 3);
    let (b, _) = reserve_pages(whole, Placement::lowest(4096).unwrap(), 6, 3);
    let flat = create(3);
    let Snapshot::Flat(flat_info) = inspect(Query::Flat {
        token: flat.token(),
        flat: flat.flat(),
    }) else {
        panic!("Flat")
    };
    for (own, other) in [(a, b), (b, a)] {
        let worker = Worker::new();
        let limits = Limits {
            scope: PendScope::Nest(NestScope::Reservation {
                id: own.reservation(),
                range: PendRange::pages(1, 1).unwrap(),
            }),
            ..pend_info(own.token()).limits
        };
        let token = accord(own.token(), worker.0, limits);
        let local = at(own, token, 1);
        assert!(matches!(
            worker.call(Action::Inspect(Query::Memo(local))),
            Ok(Response::Observation(_))
        ));
        rejected(worker.call(Action::Inspect(Query::Memo(at(own, token, 0)))));
        rejected(worker.call(Action::Inspect(Query::Memo(at(other, token, 1)))));
        // Even the actual parent's numeric token is not in this Task's ledger.
        rejected(worker.call(Action::Inspect(Query::Memo(own))));
        rejected(worker.call(Action::Release {
            token,
            nest: own.nest(),
            reservation: own.reservation(),
            expected: reservation(own).version,
        }));
        rejected(worker.call(Action::FaultRoute {
            token,
            nest: own.nest(),
            reservation: own.reservation(),
            expected: reservation(own).version,
            epoch: FaultEpoch::new(1).unwrap(),
            route: None,
        }));
        let partial = accord(
            flat.token(),
            worker.0,
            Limits {
                scope: PendScope::Pages(PendRange::pages(1, 1).unwrap()),
                ..pend_info(flat.token()).limits
            },
        );
        rejected(worker.call(Action::DeleteFlat {
            token: partial,
            flat: flat.flat(),
            expected: flat_info.version,
        }));
        assert_eq!(reservation(own).count, 3);
        assert_eq!(reservation(other).phase, ReservationPhase::Open);
        worker.stop();
    }
    let token = derive(
        a.token(),
        Limits {
            scope: PendScope::Nest(NestScope::Reservation {
                id: a.reservation(),
                range: PendRange::Inherit,
            }),
            ..pend_info(a.token()).limits
        },
    );
    narrow(
        token,
        Limits {
            scope: PendScope::Nest(NestScope::Reservation {
                id: a.reservation(),
                range: PendRange::pages(1, 1).unwrap(),
            }),
            ..pend_info(token).limits
        },
    );
    let narrowed = at(a, token, 1);
    assert_eq!(reservation(narrowed).offset, 1);
    assert_eq!(memo_status(narrowed).coordinate, 1);
    rejected(try_call(Action::Inspect(Query::Memo(at(a, token, 0)))));
    assert_eq!(reservation(narrowed).address, reservation(a).address);
    let flat_child = derive(flat.token(), pend_info(flat.token()).limits);
    narrow(
        flat_child,
        Limits {
            scope: PendScope::Pages(PendRange::pages(1, 1).unwrap()),
            ..pend_info(flat_child).limits
        },
    );
    assert_eq!(
        slot_status(SlotPosition::new(flat_child, flat.flat(), 1).unwrap()).coordinate,
        1
    );
    rejected(try_call(Action::Inspect(Query::Slot(
        SlotPosition::new(flat_child, flat.flat(), 0).unwrap(),
    ))));
    let _ = writeln!(
        Console,
        "PASS acceptance-scopes reservations=2 tasks=2 partial-delete=reject partial-release=reject partial-fault=reject narrow-fixed=1"
    );
    replacement();
    aba();
}

fn replacement() {
    let first = create(1);
    let second = create(1);
    for slot in [first, second] {
        mutate(Action::Occupy {
            slot,
            expected: slot_status(slot).version,
        });
    }
    let owner = pend_info(first.token()).owner;
    let page = slot_status(first).page;
    let (memo, _) = reserve_pages(space(), Placement::lowest(4096).unwrap(), 6, 2);
    let neighbor = at(memo, memo.token(), 1);
    insert(memo, first);
    insert(neighbor, first);
    let before = memo_status(neighbor);
    let worker = Worker::new();
    let flat_token = accord(second.token(), worker.0, pend_info(second.token()).limits);
    let source = Source::Flat {
        position: SlotPosition::new(flat_token, second.flat(), 0).unwrap(),
        expected: slot_status(second).version,
    };
    for (rights, succeeds) in [
        (NestRights::INSERT, false),
        (NestRights::INSERT | NestRights::REMOVE, true),
    ] {
        let token = accord(
            memo.token(),
            worker.0,
            Limits {
                operations: Operations::Nest(rights),
                ..pend_info(memo.token()).limits
            },
        );
        let response = worker.call(Action::Insert {
            target: at(memo, token, 0),
            expected: memo_status(memo).generation,
            source,
        });
        if succeeds {
            assert!(
                matches!(response, Ok(Response::Reply(r)) if matches!(r.outcome(), Outcome::Committed(_)))
            );
        } else {
            rejected(response);
            assert_eq!(memo_status(memo).source, Some((first.flat(), 0)));
        }
    }
    assert_eq!(memo_status(memo).source, Some((second.flat(), 0)));
    assert_eq!(memo_status(neighbor), before);
    assert_eq!(slot_status(first).page, page);
    assert_eq!(pend_info(first.token()).owner, owner);
    assert_eq!(slot_status(first).phase, SlotPhase::Resident);
    assert_eq!(slot_status(second).phase, SlotPhase::Resident);
    worker.stop();
    let _ = writeln!(
        Console,
        "PASS acceptance-replace insert-only=reject insert-remove=commit neighbor=unchanged holdings=unchanged"
    );
}

fn aba() {
    let (old, address) = reserve(space());
    let slot = create(1);
    insert(old, slot);
    let (reader, epoch) = crate::fault::start_reader(old, address, 0);
    let event = crate::fault::event(
        old,
        epoch,
        kernel_api::model::wait::Wait::At(kernel_client::call::room::clock() + 1_000_000_000),
    );
    let old_generation = event.generation;
    let old_release = release(old);
    mutate(old_release);
    assert!(matches!(join(reader).exit, Some((ExitCause::Fault, _))));
    let (new, actual) = reserve_at(space(), Placement::exact(address, 4096).unwrap(), 6);
    assert_eq!(actual, address);
    assert_ne!(old.reservation(), new.reservation());
    // A retained observation may describe the old lifecycle, never the new one.
    if let Ok(Response::Observation(o)) = try_call(Action::Inspect(Query::Memo(old))) {
        let Snapshot::Memo(s) = o.snapshot() else {
            panic!("Memo")
        };
        assert_eq!(s.reservation, old.reservation());
        assert_ne!(s.reservation, new.reservation());
    }
    rejected(try_call(Action::Inspect(Query::Memo(
        MemoPosition::new(new.token(), new.nest(), old.reservation(), 0).unwrap(),
    ))));
    rejected(try_call(old_release));
    rejected(try_call(Action::Publish {
        memo: old,
        expected: old_generation,
        maintenance: None,
    }));
    rejected(try_call(Action::Inspect(Query::FaultEvent {
        token: new.token(),
        nest: new.nest(),
        reservation: event.reservation,
        epoch,
        after: 0,
        wait: kernel_api::model::wait::Wait::Poll,
    })));
    assert_eq!(memo_status(new).phase, MemoPhase::Empty);
    let _ = writeln!(
        Console,
        "PASS acceptance-aba same-address=1 old-pend=reject old-reservation=reject old-request=reject old-event-identity=reject"
    );
}
