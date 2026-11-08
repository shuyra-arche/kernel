#[path = "../../src/metal/iommu/riscv/domain/phase.rs"]
mod phase;
use super::super::BatchId;
use phase::{DomainPhase as Phase, Event};

#[test]
fn failures_never_forge_a_closed_domain_or_accept_a_stale_fence() {
    let expected = BatchId::allocate().unwrap();
    let stale = BatchId::allocate().unwrap();
    for phase in [
        Phase::Prepared,
        Phase::Purging(expected),
        Phase::Installing,
        Phase::Starting(expected),
        Phase::Active,
        Phase::Withdrawing,
        Phase::Revoking(expected),
        Phase::Blocked,
        Phase::Detaching,
        Phase::Draining(expected),
        Phase::Stopping,
        Phase::Failed,
    ] {
        assert_eq!(phase.next(Event::Fail), Some(Phase::Failed));
        assert_eq!(phase.next(Event::Complete(stale)), None);
    }
    assert_eq!(Phase::Closed.next(Event::Fail), Some(Phase::Closed));
    assert_eq!(Phase::Failed.next(Event::Stopped), None);
    let path = [
        Event::Start(expected),
        Event::Complete(expected),
        Event::Installed(expected),
        Event::Complete(expected),
        Event::Revoke,
        Event::Submitted(expected),
        Event::Complete(expected),
        Event::Close,
        Event::Detached(expected),
        Event::Complete(expected),
        Event::Stopped,
    ];
    let mut current = Phase::Prepared;
    for event in path {
        current = current.next(event).unwrap();
    }
    assert_eq!(current, Phase::Closed);
}
