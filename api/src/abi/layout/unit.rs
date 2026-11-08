use super::{fetch_words, store_words};
use crate::{
    address::VirtAddr,
    model::{
        identity::{TaskId, TeamId},
        unit::{ControlId, ExitCause, ProgramKind, UnitCreate, UnitInfo, UnitState, UnitTarget},
    },
};
impl UnitCreate {
    pub const WIDTH: usize = 48;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let words = match self {
            Self::Team { kind, guard } => [
                0,
                match kind {
                    ProgramKind::User => 0,
                    ProgramKind::Supervisor => 1,
                },
                guard.get(),
                0,
                0,
                0,
            ],
            Self::Task {
                team,
                entry,
                args,
                count,
                stack,
            } => [
                1,
                team.map_or(0, TeamId::get),
                entry.get(),
                args.get(),
                count as usize,
                stack.get(),
            ],
        };
        let mut bytes = [0; Self::WIDTH];
        store_words(words, &mut bytes);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [tag, a, b, c, d, e] = fetch_words(bytes)?;
        match tag {
            0 if c == 0 && d == 0 && e == 0 => Some(Self::Team {
                kind: match a {
                    0 => ProgramKind::User,
                    1 => ProgramKind::Supervisor,
                    _ => return None,
                },
                guard: ControlId::new(b),
            }),
            1 if a <= isize::MAX as usize => Some(Self::Task {
                team: (a != 0).then(|| TeamId::new(a)),
                entry: VirtAddr::new(b),
                args: VirtAddr::new(c),
                count: d as u64,
                stack: VirtAddr::new(e),
            }),
            _ => None,
        }
    }
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
