#[path = "../../src/basis/room/interrupt/source/state.rs"]
mod state;
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
