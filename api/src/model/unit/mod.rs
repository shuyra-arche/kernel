//! Task and Team lifecycle values.
use crate::model::identity::{TaskId, TeamId};
mod kind;
pub use kind::ProgramKind;

/// A target is explicitly tagged; old TaskId-only encodings are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitTarget {
    Task(TaskId),
    Team(TeamId),
}
impl From<TaskId> for UnitTarget {
    fn from(id: TaskId) -> Self {
        Self::Task(id)
    }
}
impl From<TeamId> for UnitTarget {
    fn from(id: TeamId) -> Self {
        Self::Team(id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ExitCause {
    Reap = 1,
    Slay = 2,
    Cascade = 3,
    Fault = 4,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskExit {
    pub task: TaskId,
    pub cause: ExitCause,
    pub reason: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinReply {
    Pending,
    Reaped(TaskExit),
    Complete,
}
impl JoinReply {
    pub fn is_reaped(self) -> bool {
        matches!(self, Self::Reaped(_))
    }
}
