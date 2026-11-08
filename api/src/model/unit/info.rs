use crate::abi::layout::{fetch_words, store_words};
use crate::abi::register::{Decode, Wire};

use super::{ExitCause, UnitTarget};
use crate::model::identity::{TaskId, TeamId};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitQuery {
    Caller,
    Target(UnitTarget),
    Children { after: TeamId },
    Members { team: TeamId, after: TaskId },
}
impl Wire for UnitQuery {
    const WORDS: usize = 3;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        let (tag, a, b) = match *self {
            Self::Caller => (0usize, 0usize, 0usize),
            Self::Target(UnitTarget::Task(id)) => (1, id.get(), 0),
            Self::Target(UnitTarget::Team(id)) => (2, id.get(), 0),
            Self::Children { after } => (3, after.get(), 0),
            Self::Members { team, after } => (4, team.get(), after.get()),
        };
        tag.pack(words, at);
        a.pack(words, at);
        b.pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, Decode> {
        let tag = usize::unpack(words, at)?;
        let a = usize::unpack(words, at)?;
        let b = usize::unpack(words, at)?;
        if a > isize::MAX as usize || b > isize::MAX as usize {
            return Err(Decode::Invalid);
        }
        match (tag, a, b) {
            (0, 0, 0) => Ok(Self::Caller),
            (1, id, 0) if id != 0 => Ok(Self::Target(UnitTarget::Task(TaskId::new(id)))),
            (2, id, 0) if id != 0 => Ok(Self::Target(UnitTarget::Team(TeamId::new(id)))),
            (3, after, 0) => Ok(Self::Children {
                after: TeamId::new(after),
            }),
            (4, team, after) if team != 0 => Ok(Self::Members {
                team: TeamId::new(team),
                after: TaskId::new(after),
            }),
            _ => Err(Decode::Invalid),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum UnitState {
    Dormant = 0,
    Ready = 1,
    Running = 2,
    Waiting = 3,
    Frozen = 4,
    Stopping = 5,
    Exited = 6,
}
/// Snapshot only: IDs are not management authority. Results survive observation;
/// Delete expires the caller's record. Execution exit does not imply reclamation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitInfo {
    pub target: UnitTarget,
    pub team: TeamId,
    pub controller: TaskId,
    pub creator: TaskId,
    pub sire: TaskId,
    pub state: UnitState,
    pub reclaimed: bool,
    pub admitting: bool,
    pub exit: Option<(ExitCause, u64)>,
    pub result: Option<u64>,
}

impl UnitInfo {
    pub const WIDTH: usize = 112;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let (tag, id) = match self.target {
            UnitTarget::Task(id) => (0, id.get()),
            UnitTarget::Team(id) => (1, id.get()),
        };
        let (cause, reason) = self
            .exit
            .map_or((0, 0), |(cause, reason)| (cause as usize, reason as usize));
        let mut bytes = [0; Self::WIDTH];
        store_words(
            [
                tag,
                id,
                self.team.get(),
                self.controller.get(),
                self.creator.get(),
                self.sire.get(),
                self.state as usize,
                self.reclaimed as usize,
                self.admitting as usize,
                cause,
                reason,
                self.result.is_some() as usize,
                self.result.unwrap_or(0) as usize,
                0,
            ],
            &mut bytes,
        );
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [
            tag,
            id,
            team,
            controller,
            creator,
            sire,
            state,
            reclaimed,
            admitting,
            cause,
            reason,
            result,
            value,
            reserved,
        ] = fetch_words(bytes)?;
        if id == 0
            || id > isize::MAX as usize
            || reclaimed > 1
            || admitting > 1
            || result > 1
            || reserved != 0
            || result == 0 && value != 0
        {
            return None;
        }
        Some(Self {
            target: match tag {
                0 => UnitTarget::Task(TaskId::new(id)),
                1 => UnitTarget::Team(TeamId::new(id)),
                _ => return None,
            },
            team: TeamId::new(team),
            controller: TaskId::new(controller),
            creator: TaskId::new(creator),
            sire: TaskId::new(sire),
            state: match state {
                0 => UnitState::Dormant,
                1 => UnitState::Ready,
                2 => UnitState::Running,
                3 => UnitState::Waiting,
                4 => UnitState::Frozen,
                5 => UnitState::Stopping,
                6 => UnitState::Exited,
                _ => return None,
            },
            reclaimed: reclaimed != 0,
            admitting: admitting != 0,
            exit: match cause {
                0 if reason == 0 => None,
                1 => Some((ExitCause::Reap, reason as u64)),
                2 => Some((ExitCause::Slay, reason as u64)),
                3 => Some((ExitCause::Cascade, reason as u64)),
                4 => Some((ExitCause::Fault, reason as u64)),
                _ => return None,
            },
            result: (result != 0).then_some(value as u64),
        })
    }
}
