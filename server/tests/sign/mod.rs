extern crate alloc;
mod support;
pub(crate) use support::{Task, basis, metal};
#[allow(dead_code)]
#[path = "../../src/basis/unit/task/chain.rs"]
mod chain;
#[path = "../../src/basis/sign/endpoint/state.rs"]
mod state;
mod wait;
use state::Event;
use state::Rejection;
use state::State;

#[test]
fn pending_event_survives_observation_and_is_never_overwritten() {
    let initial = State::new();
    assert_eq!(initial.transition(Event::Consume(1)), Err(Rejection::Busy));
    let pending = initial.transition(Event::Trigger).unwrap();
    assert!(pending.alive() && pending.ready());
    assert!(pending.ready()); // Observation is not consumption.
    assert_eq!(pending.transition(Event::Trigger), Err(Rejection::Busy));
    let empty = pending.transition(Event::Consume(1)).unwrap();
    assert!(!empty.ready());
    assert!(empty.transition(Event::Trigger).unwrap().ready());
}
#[test]
fn close_wins_over_pending_and_rejects_late_effects() {
    for state in [
        State::new(),
        State::new().transition(Event::Trigger).unwrap(),
    ] {
        let closed = state.transition(Event::Close).unwrap();
        assert!(!closed.alive() && !closed.ready());
        assert_eq!(closed.transition(Event::Trigger), Err(Rejection::Closed));
        assert_eq!(closed.transition(Event::Consume(1)), Err(Rejection::Closed));
        assert_eq!(closed.transition(Event::Close), Ok(closed));
    }
}

#[test]
fn stale_consumption_cannot_clear_a_new_event() {
    let first = State::new().transition(Event::Trigger).unwrap();
    let second = first
        .transition(Event::Consume(1))
        .unwrap()
        .transition(Event::Trigger)
        .unwrap();
    assert_eq!(second.sequence(), Some(2));
    assert_eq!(second.transition(Event::Consume(1)), Err(Rejection::Busy));
    assert_eq!(second.transition(Event::Consume(0)), Err(Rejection::Busy));
    assert!(!second.transition(Event::Consume(2)).unwrap().ready());
}
#[test]
fn sequence_exhaustion_never_reuses_an_old_event_identity() {
    let last = State::Open {
        pending: true,
        sequence: u64::MAX,
    };
    let empty = last.transition(Event::Consume(u64::MAX)).unwrap();
    assert_eq!(empty.transition(Event::Trigger), Err(Rejection::Exhausted));
    assert_eq!(empty.transition(Event::Close), Ok(State::Closed));
}
