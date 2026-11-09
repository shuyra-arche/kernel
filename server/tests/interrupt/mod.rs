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
fn pending_before_listen_is_retained_and_only_exact_ack_rearms() {
    let mut source = State::new();
    let owner = holder(1, 2);
    let generation = source.claim(owner).unwrap();
    assert_eq!(source.claim(holder(3, 4)), Err(Error::Busy));
    source.rearm(owner, generation, 0).unwrap();
    assert!(source.armed());
    source.claimed();
    source.deliver(1).unwrap();
    let pending = source;
    assert!(!source.armed());
    assert_eq!(source.deliver(2), Err(Error::Stale));
    assert_eq!(source.rearm(owner, generation, 0), Err(Error::Stale));
    assert_eq!(source.rearm(holder(2, 2), generation, 1), Err(Error::Stale));
    assert_eq!(source, pending);
    source.rearm(owner, generation, 1).unwrap();
    assert_eq!(source.rearm(owner, generation, 1), Err(Error::Stale));
    source.claimed();
    source.deliver(2).unwrap();
    assert_eq!(source.rearm(owner, generation, 1), Err(Error::Stale));
    source.rearm(owner, generation, 2).unwrap();
}
#[test]
fn revoked_or_exited_driver_cannot_rearm_a_replacement() {
    let mut source = State::new();
    let old = holder(1, 2);
    let first = source.claim(old).unwrap();
    source.rearm(old, first, 0).unwrap();
    source.claimed();
    source.deliver(7).unwrap();
    assert!(!source.revoke(PendId::mint(4)));
    assert!(source.revoke(old.permit));
    let next = holder(2, 5);
    let second = source.claim(next).unwrap();
    assert!(second > first);
    assert!(source.owned(next));
    assert!(!source.owned(old));
    assert_eq!(source.rearm(old, first, 7), Err(Error::Stale));
    assert_eq!(source.yield_binding(old, first), Err(Error::Stale));
    assert!(
        source.rearm(next, second, 0).unwrap(),
        "retained claim must be completed by the new binding"
    );
    source.claimed();
    source.deliver(8).unwrap();
    source.yield_binding(next, second).unwrap();
    assert!(!source.owned(next));
    source.claim(next).unwrap();
    source.close();
    assert!(!source.armed());
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
