#[path = "../../src/basis/unit/task/execution/phase.rs"]
mod phase;
use phase::Phase::*;

#[test]
fn terminal_tasks_cannot_be_rescheduled_or_blocked() {
    for next in [Held, Ready, Running, Waiting, Parked] {
        assert!(!Terminating.allows(next));
        assert!(!Finished.allows(next));
    }
    assert!(Terminating.allows(Finished));
    assert!(!Finished.allows(Terminating));
    assert!(!Finished.allows(Finished));
}
#[test]
fn every_live_phase_can_be_terminated_without_skipping_ready_publication() {
    for phase in [Held, Ready, Running, Waiting, Parked] {
        assert!(phase.allows(Terminating));
    }
    assert!(!Held.allows(Running));
    assert!(!Waiting.allows(Running));
    assert!(!Parked.allows(Running));
    assert!(Waiting.allows(Ready));
    assert!(Ready.allows(Running));
    assert!(Running.allows(Waiting));
}
