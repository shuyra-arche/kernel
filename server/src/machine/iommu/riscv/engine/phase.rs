use super::super::super::BatchId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Prepared,
    StartingFaults,
    StartingCommands,
    Ready,
    Waiting(BatchId),
    StoppingCommands,
    StoppingFaults,
    Closed,
    Failed,
}

pub(super) enum Event {
    Start,
    FaultsOn,
    CommandsOn,
    Submit(BatchId),
    Complete(BatchId),
    Stop,
    CommandsOff,
    FaultsOff,
    Fail,
}

impl Phase {
    pub(super) fn transition(self, event: Event) -> Option<Self> {
        use Event::*;
        Some(match (self, event) {
            (Self::Prepared, Start) => Self::StartingFaults,
            (Self::StartingFaults, FaultsOn) => Self::StartingCommands,
            (Self::StartingCommands, CommandsOn) => Self::Ready,
            (Self::Ready, Submit(batch)) => Self::Waiting(batch),
            (Self::Waiting(expected), Complete(actual)) if expected == actual => Self::Ready,
            (Self::Prepared, Stop) => Self::Closed,
            (Self::Ready, Stop) => Self::StoppingCommands,
            (Self::StoppingCommands, CommandsOff) => Self::StoppingFaults,
            (Self::StoppingFaults, FaultsOff) => Self::Closed,
            (state, Fail) if state != Self::Closed => Self::Failed,
            _ => return None,
        })
    }
}
