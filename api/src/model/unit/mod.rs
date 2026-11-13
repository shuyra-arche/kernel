//! Task and Team lifecycle values.
mod control;
mod creation;
mod info;
mod schedule;

pub use control::{ControlId, ControlKind};
pub use creation::{CreatedUnit, UnitCreate};
pub use info::{UnitInfo, UnitQuery, UnitState};
pub use schedule::{Schedule, ScheduleInfo};

use crate::abi::register::{Decode, Wire};
use crate::model::identity::{TaskId, TeamId};

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

impl Wire for UnitTarget {
    const WORDS: usize = 2;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        let (tag, id): (usize, usize) = match self {
            Self::Task(id) => (0, id.get()),
            Self::Team(id) => (1, id.get()),
        };
        tag.pack(words, at);
        id.pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        let tag = usize::unpack(words, at)?;
        let id = usize::unpack(words, at)?;
        if id == 0 || id > isize::MAX as usize {
            return Err(Decode::Invalid);
        }
        match tag {
            0 => Ok(Self::Task(TaskId::new(id))),
            1 => Ok(Self::Team(TeamId::new(id))),
            _ => Err(Decode::Invalid),
        }
    }
}

/// The calling Task or its own Team.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitScope {
    Task,
    Team,
}

impl Wire for ExitScope {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        matches!(self, Self::Team).pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        Ok(if bool::unpack(words, at)? {
            Self::Team
        } else {
            Self::Task
        })
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
