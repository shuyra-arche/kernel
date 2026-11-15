//! Preparation decisions are pure; the atomic owner commits only against the
//! exact state it observed. Hardware effects execute after a successful claim.
use core::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum State {
    Idle,
    Preparing,
    Ready,
    Failed,
}
impl State {
    fn decode(word: u8) -> Self {
        match word {
            0 => Self::Idle,
            1 => Self::Preparing,
            2 => Self::Ready,
            3 => Self::Failed,
            _ => unreachable!("only preparation transitions encode this state"),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unsettled {
    Busy,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Claim {
    Prepare,
    Wait,
    Ready,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Request,
    Completed(Result<(), Unsettled>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Transition {
    pub next: State,
    pub decision: Claim,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct InvalidCompletion;

/// No I/O, mutation, locks or allocation. Busy preserves the obligation and
/// permits a later claimant; failure is terminal and never permits the barrier.
pub(super) fn decide(state: State, event: Event) -> Result<Transition, InvalidCompletion> {
    let (next, decision) = match (state, event) {
        (State::Idle, Event::Request) => (State::Preparing, Claim::Prepare),
        (State::Preparing, Event::Request) => (State::Preparing, Claim::Wait),
        (State::Ready, Event::Request) => (State::Ready, Claim::Ready),
        (State::Failed, Event::Request) => (State::Failed, Claim::Failed),
        (State::Preparing, Event::Completed(Ok(()))) => (State::Ready, Claim::Ready),
        (State::Preparing, Event::Completed(Err(Unsettled::Busy))) => (State::Idle, Claim::Wait),
        (State::Preparing, Event::Completed(Err(Unsettled::Failed))) => {
            (State::Failed, Claim::Failed)
        }
        (_, Event::Completed(_)) => return Err(InvalidCompletion),
    };
    Ok(Transition { next, decision })
}

pub(super) struct Preparation(AtomicU8);
impl Preparation {
    pub(super) const fn new() -> Self {
        Self(AtomicU8::new(State::Idle as u8))
    }
    pub(super) fn claim(&self) -> Claim {
        let mut observed = self.0.load(Ordering::Acquire);
        loop {
            let state = State::decode(observed);
            let proposal =
                decide(state, Event::Request).expect("request is defined for every state");
            if proposal.next == state {
                return proposal.decision;
            }
            match self.0.compare_exchange(
                observed,
                proposal.next as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return proposal.decision,
                Err(current) => observed = current,
            }
        }
    }
    /// Only the unique successful claimant calls this, exactly once after its
    /// hardware step returns. A stale/duplicate completion is a protocol defect;
    /// do not retry it against a different owner's state.
    pub(super) fn complete(&self, result: Result<(), Unsettled>) -> bool {
        let proposal =
            decide(State::Preparing, Event::Completed(result)).expect("claimed preparation");
        self.0
            .compare_exchange(
                State::Preparing as u8,
                proposal.next as u8,
                Ordering::Release,
                Ordering::Relaxed,
            )
            .expect("only the claimed preparer may publish completion");
        proposal.decision == Claim::Ready
    }
}
