#[allow(dead_code)]
#[path = "../../src/metal/generic/interrupt.rs"]
mod mechanism;
#[path = "../../src/basis/room/interrupt/source/state.rs"]
mod state;
mod metal {
    pub(crate) mod generic {
        pub(crate) use crate::mechanism as interrupt;
    }
}
#[path = "../../src/basis/room/interrupt/progress.rs"]
mod progress;
use kernel_api::model::identity::{PendId, TaskId};
use state::{Action, Error, Holder, State};
fn holder(actor: usize, permit: usize) -> Holder {
    Holder {
        actor: TaskId::new(actor),
        permit: PendId::mint(permit),
    }
}
#[test]
fn consumption_is_exact_and_cancellation_does_not_complete_hardware() {
    let mut s = State::<()>::new();
    assert!(!s.bound());
    s.attach(holder(1, 2), (), 1).unwrap();
    assert!(!s.armed());
    assert_eq!(s.attach(holder(3, 4), (), 2), Err(Error::Busy));
    assert_eq!(s.consume(0), Action::None);
    assert_eq!(s.consume(1), Action::Open);
    assert!(s.armed());
    assert!(s.claimed());
    s.deliver(2).unwrap();
    assert_eq!(s.consume(1), Action::None);
    assert_eq!(s.consume(2), Action::Complete);
    assert_eq!(s.consume(2), Action::None);
    s.claimed();
    s.deliver(3).unwrap();
    s.detach();
    assert!(!s.bound());
    assert_eq!(s.consume(3), Action::None);
    s.attach(holder(3, 4), (), 4).unwrap();
    assert_eq!(s.consume(3), Action::None);
    assert_eq!(
        s.consume(4),
        Action::Complete,
        "replacement retains old controller claim"
    );
}
#[test]
fn initial_inspection_and_rebind_never_reuse_a_receipt() {
    let mut s = State::<()>::new();
    s.attach(holder(1, 2), (), 1).unwrap();
    s.detach();
    s.attach(holder(2, 3), (), 2).unwrap();
    assert!(!s.owned(holder(1, 2)));
    assert!(s.owned(holder(2, 3)));
    assert_eq!(s.consume(1), Action::None);
    assert_eq!(s.consume(2), Action::Open);
}
#[test]
fn ledger_names_do_not_alias_sources_across_controller_domains() {
    use kernel_api::boot::ledger::Name;
    let first = Name::Interrupt {
        controller: 0xc000000,
        source: 10,
    };
    let second = Name::Interrupt {
        controller: 0xd000000,
        source: 10,
    };
    let mut bytes = [0; Name::WIDTH];
    first.store(&mut bytes);
    assert_eq!(Name::fetch(&bytes), Some(first));
    assert_ne!(Name::fetch(&bytes), Some(second));
    bytes[2] = 1;
    assert_eq!(Name::fetch(&bytes), None);
}

#[test]
fn receiver_lifetime_follows_binding_and_claim_survives_detachment() {
    use std::sync::Arc;
    let receiver = Arc::new(());
    let mut s = State::new();
    assert_eq!(
        s.attach(holder(1, 2), receiver.clone(), 0),
        Err(Error::Stale)
    );
    assert!(s.receiver().is_none());
    assert_eq!(Arc::strong_count(&receiver), 1);
    s.attach(holder(1, 2), receiver.clone(), 1).unwrap();
    assert_eq!(Arc::strong_count(&receiver), 2);
    assert!(Arc::ptr_eq(s.receiver().unwrap(), &receiver));
    assert!(s.take_notice());
    assert!(!s.take_notice());
    s.request_notice();
    assert!(s.take_notice());
    assert_eq!(s.admit(), Err(Error::Busy));
    assert_eq!(s.consume(1), Action::Open);
    assert!(s.claimed());
    s.deliver(2).unwrap();
    assert_eq!(s.deliver(3), Err(Error::Stale));
    s.detach();
    assert!(s.receiver().is_none());
    assert_eq!(Arc::strong_count(&receiver), 1);
    s.attach(holder(3, 4), receiver.clone(), 3).unwrap();
    assert_eq!(s.consume(2), Action::None);
    assert_eq!(s.consume(3), Action::Complete);
}

#[test]
fn controller_claims_survive_failed_admission_and_move_only_once() {
    use mechanism::Irq;
    use progress::{BATCH, Claim, Progress};
    let mut work = Progress::new();
    for line in 1..=BATCH {
        assert!(work.vacant());
        work.retain(Claim {
            irq: Irq {
                controller: 0xc000000,
                line: line as u32,
            },
        });
    }
    assert!(
        !work.vacant(),
        "never issue another hardware claim without storage"
    );
    // A rejected COMMIT makes no software delivery or completion: the owner
    // keeps all four physical claims while other work requests a scan.
    work.refresh(64);
    assert!(work.pending());
    let claims = work.take();
    assert_eq!(
        claims
            .into_iter()
            .flatten()
            .map(|c| c.irq.line)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(work.take().iter().all(Option::is_none));
    assert!(work.vacant());
}

#[test]
fn repeated_requests_do_not_starve_later_sources_or_expand_one_turn() {
    use progress::{BATCH, Progress};
    let mut work = Progress::new();
    let mut visited = [0; 64];
    for _ in 0..16 {
        if visited == [0; 64] {
            work.refresh(64);
        }
        work.request(0); // one busy source cannot restart a full scan
        for _ in 0..BATCH {
            visited[work.next(64).unwrap()] += 1;
        }
    }
    assert_eq!(visited, [1; 64]);
    let mut left = 0;
    while work.next(64).is_some() {
        left += 1;
    }
    assert_eq!(left, 1);
    assert!(!work.pending());
    assert_eq!(work.next(0), None);
    work.refresh(2);
    let mut next = [work.next(2).unwrap(), work.next(2).unwrap()];
    next.sort();
    assert_eq!(next, [0, 1]);
    assert_eq!(work.next(2), None);
    work.request(7);
    work.request(7);
    assert_eq!(work.next(64), Some(7));
    assert_eq!(work.next(64), None, "coalesced work has one owner");
}
