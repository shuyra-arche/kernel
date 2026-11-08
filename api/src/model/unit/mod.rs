//! Task and Team lifecycle values.
mod control;
mod creation;
mod info;

pub use control::{ControlId, ControlKind};
pub use creation::{ProgramKind, UnitCreate};
pub use info::{UnitInfo, UnitQuery, UnitState};

use crate::abi::register::{Decode, Wire};
use crate::abi::reply::FromPair;
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

impl FromPair for crate::model::unit::UnitTarget {
    fn from_pair(kind: usize, id: usize) -> Self {
        assert!(
            id != 0 && id <= isize::MAX as usize,
            "invalid Unit Create identity"
        );
        match kind {
            0 => Self::Task(TaskId::new(id)),
            1 => Self::Team(TeamId::new(id)),
            _ => panic!("invalid Unit Create kind"),
        }
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
