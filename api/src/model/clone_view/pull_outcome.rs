use crate::model::identity::{PendId, TaskId};

/// One independently owned delivery. Open its read-only Pend through Share;
/// closing the originating queue does not revoke this copied payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PullOutcome {
    pub token: PendId,
    pub len: usize,
    pub sender: TaskId,
}
