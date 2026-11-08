use super::{ExitCause, UnitTarget};
use crate::model::identity::{TaskId, TeamId};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitQuery {
    Caller,
    Target(UnitTarget),
    Children { after: TeamId },
    Members { team: TeamId, after: TaskId },
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
