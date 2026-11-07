//! Bounded message queues and counted notifications.
use crate::model::clone_view::{Oversize, PullOutcome};
use crate::model::identity::{Mark, PendId, TaskId, VirtAddr};
use crate::model::wait::Wait;

use crate::model::clone_view::{CloneCondition, CloneLimits};
pub type CloneResult<T> = Result<T, erra::Error<CloneFail>>;

#[kernel_api_macros::envcall(class = 5, call = CloneCall)]
pub mod operations {
    #[error_domain]
    pub enum CloneFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
        Gone = -6,
    }
    #[operation(slot = 0, output = super::PendId, error = CloneFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Unseal {
        pub mark: super::Mark,
        pub limits: super::CloneLimits,
    }
    #[operation(slot = 1, output = (), error = CloneFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Seal {
        pub token: super::PendId,
    }
    #[operation(slot = 2, output = (), error = CloneFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Push {
        pub token: super::PendId,
        pub msg: super::VirtAddr,
        pub len: usize,
    }
    #[operation(slot = 3, output = super::PullOutcome, error = CloneFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Pull {
        pub token: super::PendId,
        pub buf: super::VirtAddr,
        pub max: usize,
        pub oversize: super::Oversize,
    }
    #[operation(slot = 4, output = (usize, super::TaskId, usize), error = CloneFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Peek {
        pub token: super::PendId,
    }
    #[operation(slot = 5, output = bool, error = CloneFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Wait {
        pub token: super::PendId,
        pub condition: super::CloneCondition,
        pub millis: super::Wait,
    }
}
pub use operations::{CloneCall, CloneFail};
