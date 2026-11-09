//! Unit creation snapshots and execution privilege.

use crate::abi::layout::{fetch_words, store_words};
use crate::abi::register::{Decode, Wire};

use super::ControlId;
use crate::{address::VirtAddr, model::identity::TeamId};

/// Snapshotted before allocation. Tasks are created dormant and require Resume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitCreate {
    Team {
        kind: ProgramKind,
        guard: ControlId,
        /// Consume an unbound Internal Nest root; None allocates a new Nest.
        nest: Option<crate::model::identity::PendId>,
    },
    /// None selects the caller's Team; other Teams require management authority.
    Task {
        team: Option<TeamId>,
        entry: VirtAddr,
        args: VirtAddr,
        count: u64,
        stack: VirtAddr,
    },
}

impl UnitCreate {
    pub const WIDTH: usize = 48;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let words = match self {
            Self::Team { kind, guard, nest } => [
                0,
                match kind {
                    ProgramKind::User => 0,
                    ProgramKind::Supervisor => 1,
                },
                guard.get(),
                nest.map_or(0, crate::model::identity::PendId::get),
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
            0 if c <= isize::MAX as usize && d == 0 && e == 0 => Some(Self::Team {
                kind: match a {
                    0 => ProgramKind::User,
                    1 => ProgramKind::Supervisor,
                    _ => return None,
                },
                guard: ControlId::new(b),
                nest: (c != 0).then(|| crate::model::identity::PendId::new(c)),
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

/// Execution privilege selected by the Team creation authority.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgramKind {
    /// User pages carry the U bit.
    User,
    /// Supervisor pages have the U bit clear.
    Supervisor,
}

impl Wire for ProgramKind {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = match self {
            ProgramKind::User => 0,
            ProgramKind::Supervisor => 1,
        };
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        match v {
            0 => Ok(ProgramKind::User),
            1 => Ok(ProgramKind::Supervisor),
            _ => Err(Decode::Invalid),
        }
    }
}
