#[path = "../../src/basis/room/interrupt/source/state.rs"]
mod state;
use kernel_api::model::identity::{PendId, TaskId};
use state::{Error, Holder, State};
fn holder(actor: usize, permit: usize) -> Holder {
    Holder {
        actor: TaskId::new(actor),
        permit: PendId::mint(permit),
    }
}
#[test]
fn consumption_is_exact_and_cancellation_does_not_complete_hardware() {
    let mut s = State::new();
    assert!(!s.bound());
    s.claim(holder(1, 2)).unwrap();
    assert!(s.bound());
    s.begin(1);
    assert!(!s.armed());
    assert_eq!(s.claim(holder(3, 4)), Err(Error::Busy));
    assert_eq!(s.consume(0), None);
    assert_eq!(s.consume(1), Some(false));
    assert!(s.armed());
    assert!(s.claimed());
    s.deliver(2).unwrap();
    assert_eq!(s.consume(1), None);
    assert_eq!(s.consume(2), Some(true));
    assert_eq!(s.consume(2), None);
    s.claimed();
    s.deliver(3).unwrap();
    s.close();
    assert!(!s.bound());
    assert_eq!(s.consume(3), None);
    s.claim(holder(3, 4)).unwrap();
    s.begin(4);
    assert_eq!(s.consume(3), None);
    assert_eq!(
        s.consume(4),
        Some(true),
        "replacement retains old controller claim"
    );
}
#[test]
fn initial_inspection_and_rebind_never_reuse_a_receipt() {
    let mut s = State::new();
    let first = s.claim(holder(1, 2)).unwrap();
    s.begin(1);
    s.close();
    let second = s.claim(holder(2, 3)).unwrap();
    s.begin(2);
    assert!(second > first);
    assert!(!s.owned(holder(1, 2)));
    assert!(s.owned(holder(2, 3)));
    assert_eq!(s.consume(1), None);
    assert_eq!(s.consume(2), Some(false));
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
