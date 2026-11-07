use crate::model::identity::TaskId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullOutcome {
    Received { len: usize, sender: TaskId },
    Discarded { len: usize, sender: TaskId },
}
