mod ranges;
#[path = "../../src/basis/pend/state.rs"]
mod state;
#[path = "../../src/basis/pend/window.rs"]
mod window;
use kernel_api::model::modifiable::Modifiable::No;
use kernel_api::model::modifiable::Modifiable::Yes;
use state::Event::*;
use state::Failure;
use state::Phase;
use state::State;

#[test]
fn mutable_parent_is_suspended_until_every_reader_returns() {
    let root = State::new(Yes);
    let one = root.transition(Borrow(No)).unwrap();
    let two = one.transition(Borrow(No)).unwrap();
    assert!(!one.usable() && !two.usable());
    assert_eq!(two.transition(Borrow(Yes)), Err(Failure::Busy));
    assert_eq!(two.transition(Narrow(No)), Err(Failure::Busy));
    let one = two.transition(Return(No)).unwrap();
    assert!(!one.usable());
    assert_eq!(one.transition(Return(No)), Ok(root));
    assert_eq!(root.transition(Return(No)), Err(Failure::Denied));
}
#[test]
fn shared_parent_remains_readable_but_cannot_produce_a_writer() {
    let root = State::new(No);
    assert_eq!(root.transition(Borrow(Yes)), Err(Failure::Denied));
    assert_eq!(root.transition(Narrow(Yes)), Err(Failure::Denied));
    assert!(root.transition(Borrow(No)).unwrap().usable());
}
#[test]
fn mutable_reborrow_excludes_every_sibling() {
    let root = State::new(Yes);
    let borrowed = root.transition(Borrow(Yes)).unwrap();
    assert!(!borrowed.usable());
    for access in [No, Yes] {
        assert_eq!(borrowed.transition(Borrow(access)), Err(Failure::Busy));
    }
    assert_eq!(borrowed.transition(Return(No)), Err(Failure::Denied));
    assert_eq!(borrowed.transition(Return(Yes)), Ok(root));
}
#[test]
fn retirement_never_resumes_access_and_waits_for_children() {
    let borrowed = State::new(Yes).transition(Borrow(No)).unwrap();
    let retiring = borrowed.transition(Retire).unwrap();
    assert_eq!(retiring.phase, Phase::Retiring);
    assert!(!retiring.usable());
    assert_eq!(retiring.transition(Settle), Err(Failure::Busy));
    assert_eq!(retiring.transition(Borrow(No)), Err(Failure::Dead));
    let returned = retiring.transition(Return(No)).unwrap();
    assert!(!returned.usable());
    let dead = returned.transition(Settle).unwrap();
    assert_eq!(dead.phase, Phase::Dead);
    assert_eq!(dead.transition(Retire), Ok(dead));
    assert!(!dead.usable());
}

#[test]
fn cancellation_and_settlement_require_different_proofs() {
    let active = State::new(Yes);
    assert_eq!(active.transition(Settle), Err(Failure::Denied));
    assert_eq!(active.transition(Cancel).unwrap().phase, Phase::Dead);
    let borrowed = active.transition(Borrow(No)).unwrap();
    assert_eq!(borrowed.transition(Cancel), Err(Failure::Busy));
    let retiring = active.transition(Retire).unwrap();
    assert_eq!(retiring.transition(Cancel), Err(Failure::Denied));
    assert_eq!(retiring.transition(Settle).unwrap().phase, Phase::Dead);
}
