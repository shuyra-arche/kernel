use crate::model::hole::HoleCondition;
use crate::model::identity::{PieToken, TaskId, TeamId};
use crate::model::pole::Bit;
use crate::model::unit::UnitTarget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToleEventSource {
    /// Kernel device retirement, independently of the requesting Unit lifetime.
    DeviceRetired {
        device: usize,
    },
    /// First hardware queue fault; observation does not clear or settle it.
    DeviceFault {
        device: usize,
    },
    Hole {
        token: PieToken,
        condition: HoleCondition,
    },
    Pole {
        token: PieToken,
        bit: Bit,
    },
    Nole {
        token: PieToken,
    },
    Join {
        target: UnitTarget,
    },
    Inspect {
        task: TaskId,
        token: PieToken,
    },
}

impl ToleEventSource {
    pub fn words(self) -> [usize; 3] {
        match self {
            Self::DeviceRetired { device } => [7, device, 0],
            Self::DeviceFault { device } => [8, device, 0],
            Self::Hole { token, condition } => [1, token.get(), condition as usize],
            Self::Join {
                target: UnitTarget::Task(task),
            } => [2, task.get(), 0],
            Self::Join {
                target: UnitTarget::Team(team),
            } => [3, team.get(), 0],
            Self::Inspect { task, token } => [4, task.get(), token.get()],
            Self::Pole { token, bit } => [5, token.get(), bit.index()],
            Self::Nole { token } => [6, token.get(), 0],
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
                    0 => HoleCondition::Pull,
                    1 => HoleCondition::Push,
                    2 => HoleCondition::Empty,
                    3 => HoleCondition::Signal,
                    _ => return None,
                };
                Some(Self::Hole {
                    token: PieToken::new(token),
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
                token: PieToken::new(token),
            }),
            [5, token, bit] if token != 0 => Some(Self::Pole {
                token: PieToken::new(token),
                bit: Bit::of(bit)?,
            }),
            [6, token, 0] if token != 0 => Some(Self::Nole {
                token: PieToken::new(token),
            }),
            _ => None,
        }
    }
}
