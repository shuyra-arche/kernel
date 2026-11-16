//! Execution, stable result records and independent administrative qualifications.
use crate::{
    address::VirtAddr,
    model::{
        identity::{TaskId, TeamId},
        unit::{ControlId, CreatedUnit, ExitScope, UnitQuery, UnitTarget},
    },
};
pub type UnitResult<T> = Result<T, erra::Error<UnitFail>>;
#[kernel_api_macros::envcall(class = 1, call = UnitCall)]
pub mod operations {
    #[error_domain]
    pub enum UnitFail {
        Denied = -1,
        #[busy]
        Busy = -2,
        OutOfMemory = -3,
        BadImage = -4,
        BadEntry = -5,
    }
    /// Snapshot UnitCreate before allocating. Team/Task are distinct tagged variants.
    #[operation(slot = 0, output = super::CreatedUnit, error = UnitFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create {
        pub request: super::VirtAddr,
    }
    /// Delete only a completed record or retired Team; never implies Stop.
    #[operation(slot = 1, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub target: super::UnitTarget,
    }
    #[operation(slot = 2, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Resume {
        pub target: super::UnitTarget,
    }
    #[operation(slot = 3, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Freeze {
        pub target: super::UnitTarget,
    }
    /// Request termination; completion and reclamation are observed separately.
    #[operation(slot = 4, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Stop {
        pub target: super::UnitTarget,
    }
    /// Team scope requires the caller's Team to have enabled member exit.
    #[operation(slot = 5, nonreturning)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Exit {
        pub value: u64,
        pub reason: u64,
        pub note: super::VirtAddr,
        pub len: usize,
        pub scope: super::ExitScope,
    }
    /// Permanently close Team member admission without stopping existing members.
    #[operation(slot = 6, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Finish {
        pub team: super::TeamId,
    }
    /// Write UnitInfo; false marks enumeration end, with no buffer write.
    #[operation(slot = 7, output = bool, error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub query: super::UnitQuery,
        pub out: super::VirtAddr,
    }
    #[operation(slot = 8, output = super::ControlId, error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Accord {
        pub authority: super::ControlId,
        pub target: super::TaskId,
        pub delegable: bool,
    }
    #[operation(slot = 9, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Revoke {
        pub authority: super::ControlId,
        pub target: super::TaskId,
        pub branch: super::ControlId,
    }
    #[operation(slot = 10, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Return {
        pub authority: super::ControlId,
    }
    /// Admit a dormant or fully frozen Task after resource preparation.
    /// Requires Build qualification and control of Task.
    #[operation(slot = 11, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Admit {
        pub task: super::TaskId,
        pub schedule: super::VirtAddr,
    }
    /// Withdraw a reservation before its first Resume. Never resets a used budget.
    #[operation(slot = 12, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Evict {
        pub task: super::TaskId,
    }
    /// Write a ScheduleInfo for a controlled admitted Task.
    #[operation(slot = 13, output = (), error = UnitFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Profile {
        pub task: super::TaskId,
        pub out: super::VirtAddr,
    }
}
pub use operations::{UnitCall, UnitFail};
pub const NOTE_MAX: usize = 128;
