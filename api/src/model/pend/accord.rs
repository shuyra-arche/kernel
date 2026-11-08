use super::PendRange;
use crate::model::{
    identity::{Mark, PendId, TaskId},
    modifiable::Modifiable,
};

/// A local derivation and a delegation use the same operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccordTarget {
    Caller,
    Task(TaskId),
}

/// Immutable request snapshot consumed by Pend.Accord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccordRequest {
    pub src: PendId,
    pub target: AccordTarget,
    pub subset: Modifiable,
    /// NONE inherits the parent's mark.
    pub mark: Mark,
    pub range: PendRange,
}
