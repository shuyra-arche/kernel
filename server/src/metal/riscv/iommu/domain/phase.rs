use crate::metal::iommu::BatchId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DomainPhase {
    Prepared,
    Purging(BatchId),
    Installing,
    Starting(BatchId),
    Active,
    Withdrawing,
    Revoking(BatchId),
    Blocked,
    Detaching,
    Draining(BatchId),
    Closed,
    Failed,
}

pub(super) enum Event {
    Start(BatchId),
    Installed(BatchId),
    Complete(BatchId),
    Revoke,
    Submitted(BatchId),
    Close,
    Detached(BatchId),
    Fail,
}

impl DomainPhase {
    pub(super) fn next(self, event: Event) -> Option<Self> {
        use Event::*;
        Some(match (self, event) {
            (Self::Closed, Fail) => Self::Closed,
            (_, Fail) => Self::Failed,
            (Self::Prepared, Start(batch)) => Self::Purging(batch),
            (Self::Purging(expected), Complete(actual)) if expected == actual => Self::Installing,
            (Self::Installing, Installed(batch)) => Self::Starting(batch),
            (Self::Starting(expected), Complete(actual)) if expected == actual => Self::Active,
            (Self::Active, Revoke) => Self::Withdrawing,
            (Self::Withdrawing, Submitted(batch)) => Self::Revoking(batch),
            (Self::Revoking(expected), Complete(actual)) if expected == actual => Self::Blocked,
            (Self::Blocked, Close) => Self::Detaching,
            (Self::Detaching, Detached(batch)) => Self::Draining(batch),
            (Self::Draining(expected), Complete(actual)) if expected == actual => Self::Closed,
            _ => return None,
        })
    }
}
