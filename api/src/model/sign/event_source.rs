use super::SignId;
use crate::model::clone_view::CloneCondition;
use crate::model::identity::{PendId, TaskId, TeamId};
use crate::model::unit::UnitTarget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignSource {
    /// Kernel device retirement, independently of the requesting Unit lifetime.
    DeviceRetired {
        device: usize,
    },
    /// First hardware queue fault; observation does not clear or settle it.
    DeviceFault {
        device: usize,
    },
    Clone {
        token: PendId,
        condition: CloneCondition,
    },
    Sign {
        token: SignId,
    },
    Join {
        target: UnitTarget,
    },
    Inspect {
        task: TaskId,
        token: PendId,
    },
}

impl SignSource {
    pub fn words(self) -> [usize; 3] {
        match self {
            Self::DeviceRetired { device } => [7, device, 0],
            Self::DeviceFault { device } => [8, device, 0],
            Self::Clone { token, condition } => [1, token.get(), condition as usize],
            Self::Join {
                target: UnitTarget::Task(task),
            } => [2, task.get(), 0],
            Self::Join {
                target: UnitTarget::Team(team),
            } => [3, team.get(), 0],
            Self::Inspect { task, token } => [4, task.get(), token.get()],
            Self::Sign { token } => [6, token.get(), 0],
        }
    }

    pub fn of(words: [usize; 3]) -> Option<Self> {
        match words {
            [8, device, 0] if device != 0 && device <= isize::MAX as usize => {
                Some(Self::DeviceFault { device })
            }
            [7, device, 0] if device != 0 && device <= isize::MAX as usize => {
                Some(Self::DeviceRetired { device })
            }
            [1, token, condition] if token != 0 => {
                let condition = match condition {
                    0 => CloneCondition::Pull,
                    1 => CloneCondition::Push,
                    2 => CloneCondition::Empty,
                    _ => return None,
                };
                Some(Self::Clone {
                    token: PendId::new(token),
                    condition,
                })
            }
            [2, task, 0] => Some(Self::Join {
                target: UnitTarget::Task(TaskId::new(task)),
            }),
            [3, team, 0] => Some(Self::Join {
                target: UnitTarget::Team(TeamId::new(team)),
            }),
            [4, task, token] if token != 0 => Some(Self::Inspect {
                task: TaskId::new(task),
                token: PendId::new(token),
            }),
            [6, token, 0] if token != 0 => Some(Self::Sign {
                token: SignId::new(token),
            }),
            _ => None,
        }
    }
}
