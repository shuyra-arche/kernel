use crate::abi::register::{Decode, Wire};

use crate::model::clone_view::CloneCondition;
use crate::model::identity::PendId;
use crate::model::identity::TaskId;
use crate::model::identity::TeamId;
use crate::model::unit::UnitTarget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignSource {
    /// Retained external mapping completion. Requires mapping-scoped Nest INSPECT
    /// for this identity/window; the numeric mapping is only a consistency check.
    MappingRetired {
        token: PendId,
        mapping: crate::model::cell::MappingId,
    },
    /// Retained completion of one exact publication generation, authorized by a
    /// mapping-scoped Nest INSPECT grant for the same identity/window.
    MappingPublished {
        token: PendId,
        mapping: crate::model::cell::MappingId,
    },
    /// Oldest unattributed fault retained by this binding; never a reclaim proof.
    NestFault {
        token: PendId,
    },
    /// Context and independent platform proof have both completed.
    NestClosed {
        token: PendId,
    },
    Clone {
        token: PendId,
        condition: CloneCondition,
    },
    Sign {
        token: PendId,
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
            Self::MappingRetired { token, mapping } => [9, token.get(), mapping.get()],
            Self::MappingPublished { token, mapping } => [11, token.get(), mapping.get()],
            Self::NestFault { token } => [12, token.get(), 0],
            Self::NestClosed { token } => [13, token.get(), 0],
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
            [tag, token, mapping]
                if matches!(tag, 9 | 11)
                    && token != 0
                    && mapping != 0
                    && token <= isize::MAX as usize
                    && mapping <= isize::MAX as usize =>
            {
                let token = PendId::new(token);
                let mapping = crate::model::cell::MappingId::new(mapping);
                Some(if tag == 9 {
                    Self::MappingRetired { token, mapping }
                } else {
                    Self::MappingPublished { token, mapping }
                })
            }
            [12, token, 0] if token > 0 && token <= isize::MAX as usize => Some(Self::NestFault {
                token: PendId::new(token),
            }),
            [13, token, 0] if token > 0 && token <= isize::MAX as usize => Some(Self::NestClosed {
                token: PendId::new(token),
            }),
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
                token: PendId::new(token),
            }),
            _ => None,
        }
    }
}

impl Wire for SignSource {
    const WORDS: usize = 3;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        for word in self.words() {
            word.pack(words, at);
        }
    }

    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        let mut source = [0; 3];
        for word in &mut source {
            *word = usize::unpack(words, at)?;
        }
        Self::of(source).ok_or(Decode::Invalid)
    }
}
