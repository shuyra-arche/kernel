//! Unit creation snapshots and execution privilege.

use crate::abi::layout::{fetch_words, store_words};
use crate::abi::register::{Decode, Wire};

use super::ControlId;
use crate::{
    address::VirtAddr,
    model::identity::{PendId, TaskId, TeamId},
};

/// A new Team includes its live, limited CPU Whole grant. Tasks receive no default grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreatedUnit {
    Task(TaskId),
    Team { team: TeamId, nest: PendId },
}

impl CreatedUnit {
    pub fn decode(kind: usize, id: usize, nest: usize) -> Option<Self> {
        if id == 0 || id > isize::MAX as usize {
            return None;
        }
        match kind {
            0 if nest == 0 => Some(Self::Task(TaskId::new(id))),
            1 if nest != 0 && nest <= isize::MAX as usize => Some(Self::Team {
                team: TeamId::new(id),
                nest: PendId::new(nest),
            }),
            _ => None,
        }
    }
}

impl crate::abi::reply::FromTriple for CreatedUnit {
    fn from_triple(kind: usize, id: usize, nest: usize) -> Self {
        Self::decode(kind, id, nest).expect("invalid created Unit reply")
    }
}

/// Snapshotted before allocation. Tasks are created dormant and require Resume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitCreate {
    Team {
        kind: ProgramKind,
        guard: ControlId,
        /// Adopt an empty, exclusive Internal Nest root; None allocates a new Nest.
        /// Success retains its identity, consumes ATTACH/DELETE and returns limited Whole authority.
        nest: Option<crate::model::identity::PendId>,
        /// Permit members to exit this Team through Unit.Exit.
        member_exit: bool,
    },
    /// None selects the caller's Team; other Teams require management authority.
    ///
    /// On RV64, `a0` points to the copied argument words and `a1` holds their
    /// count. The words occupy the end of the writable stack, so its exclusive
    /// upper bound is `a0 + a1 * 8`, including when `a1` is zero. The initial
    /// `sp` is `a0` rounded down to a 16-byte boundary.
    Task {
        team: Option<TeamId>,
        entry: VirtAddr,
        args: VirtAddr,
        count: u64,
        /// Stack size in bytes, rounded up to a base page. Zero selects the
        /// default size. The writable stack has an unmapped guard below it.
        stack: VirtAddr,
    },
}

impl UnitCreate {
    pub const WIDTH: usize = 48;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let words = match self {
            Self::Team {
                kind,
                guard,
                nest,
                member_exit,
            } => [
                0,
                match kind {
                    ProgramKind::User => 0,
                    ProgramKind::Supervisor => 1,
                },
                guard.get(),
                nest.map_or(0, crate::model::identity::PendId::get),
                member_exit as usize,
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
            0 if c <= isize::MAX as usize && d <= 1 && e == 0 => Some(Self::Team {
                kind: match a {
                    0 => ProgramKind::User,
                    1 => ProgramKind::Supervisor,
                    _ => return None,
                },
                guard: ControlId::new(b),
                nest: (c != 0).then(|| crate::model::identity::PendId::new(c)),
                member_exit: d == 1,
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
