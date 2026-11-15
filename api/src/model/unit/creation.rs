//! Unit creation snapshots. Public Team construction always selects User.

use crate::abi::layout::{fetch_words, store_words};

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
        guard: ControlId,
        /// Adopt an empty, exclusive Internal Nest root; None allocates a new Nest.
        /// Success retains its identity, consumes ATTACH/DELETE and returns limited Whole authority.
        nest: Option<crate::model::identity::PendId>,
        /// Permit members to exit this Team through Unit.Exit.
        member_exit: bool,
    },
    /// None selects the caller's Team; other Teams require management authority.
    /// Memory is already prepared in the target Nest. No words are copied from
    /// the creator and the Task acquires no ownership of the user stack.
    Task {
        team: Option<TeamId>,
        start: TaskStart,
    },
}

impl UnitCreate {
    pub const WIDTH: usize = 48;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let words = match self {
            Self::Team {
                guard,
                nest,
                member_exit,
            } => [
                0,
                0, // Retired privilege selector: only canonical zero is accepted.
                guard.get(),
                nest.map_or(0, crate::model::identity::PendId::get),
                member_exit as usize,
                0,
            ],
            Self::Task { team, start } => [
                2, // Tag 1 was the retired Kernel-allocated stack descriptor.
                team.map_or(0, TeamId::get),
                start.entry.get(),
                start.stack_pointer.get(),
                start.arguments.get(),
                start.argument_count,
            ],
        };
        let mut bytes = [0; Self::WIDTH];
        store_words(words, &mut bytes);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [tag, a, b, c, d, e] = fetch_words(bytes)?;
        match tag {
            0 if a == 0 && c <= isize::MAX as usize && d <= 1 && e == 0 => Some(Self::Team {
                guard: ControlId::new(b),
                nest: (c != 0).then(|| crate::model::identity::PendId::new(c)),
                member_exit: d == 1,
            }),
            2 if a <= isize::MAX as usize => Some(Self::Task {
                team: (a != 0).then(|| TeamId::new(a)),
                start: TaskStart {
                    entry: VirtAddr::new(b),
                    stack_pointer: VirtAddr::new(c),
                    arguments: VirtAddr::new(d),
                    argument_count: e,
                },
            }),
            _ => None,
        }
    }
}

/// RV64 initial user registers, versioned by the enclosing ABI and descriptor
/// tag. The Kernel owns all privilege, address-space and trap-context state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskStart {
    pub entry: VirtAddr,
    pub stack_pointer: VirtAddr,
    pub arguments: VirtAddr,
    pub argument_count: usize,
}
impl TaskStart {
    pub const MAX_ARGUMENTS: usize = 64;
    /// Pure geometry validation. Authorization and current target-Nest mappings
    /// are checked separately by the Kernel at publication. SP is an exclusive
    /// boundary, so the user ceiling is allowed; the first store is below it.
    pub fn valid(self, user_end: usize) -> bool {
        let pc = self.entry.get();
        let sp = self.stack_pointer.get();
        let args = self.arguments.get();
        self.argument_count <= Self::MAX_ARGUMENTS
            && pc != 0
            && pc.is_multiple_of(2)
            && pc.checked_add(2).is_some_and(|end| end <= user_end)
            && sp != 0
            && sp <= user_end
            && sp.is_multiple_of(16)
            && args <= user_end
            && (self.argument_count == 0 || (args != 0 && args.is_multiple_of(8)))
            && self
                .argument_count
                .checked_mul(8)
                .and_then(|len| args.checked_add(len))
                .is_some_and(|end| end <= user_end)
    }
}
