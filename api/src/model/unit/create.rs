use super::{ControlId, ProgramKind};
use crate::{address::VirtAddr, model::identity::TeamId};
/// Snapshotted before allocation. Tasks are created dormant and require Resume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitCreate {
    Team {
        kind: ProgramKind,
        guard: ControlId,
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
