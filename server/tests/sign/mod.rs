#[path = "../../src/core/work/sign/endpoint/state.rs"]
mod state;
use state::{Event, Rejection, State};

#[test]
fn pending_event_survives_observation_and_is_never_overwritten() {
    let initial = State::new();
    assert_eq!(initial.transition(Event::Consume), Err(Rejection::Busy));
    let pending = initial.transition(Event::Trigger).unwrap();
    assert!(pending.alive() && pending.ready());
    assert!(pending.ready()); // Observation is not consumption.
    assert_eq!(pending.transition(Event::Trigger), Err(Rejection::Busy));
    let empty = pending.transition(Event::Consume).unwrap();
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
        assert_eq!(closed.transition(Event::Consume), Err(Rejection::Closed));
        assert_eq!(closed.transition(Event::Close), Ok(closed));
    }
}
