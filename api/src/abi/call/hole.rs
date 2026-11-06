//! Bounded message queues and counted notifications.
use crate::model::hole::{Oversize, PullOutcome};
use crate::model::identity::{Mark, PieToken, TaskId, VirtAddr};
use crate::model::wait::Wait;

use crate::model::hole::{HoleCondition, HoleLimits};
pub type HoleResult<T> = Result<T, erra::Error<HoleFail>>;

#[kernel_api_macros::envcall(class = 5, call = HoleCall)]
pub mod operations {
    #[error_domain]
    pub enum HoleFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
        Gone = -6,
    }
    #[operation(slot = 0, output = super::PieToken, error = HoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Unseal {
        pub mark: super::Mark,
        pub limits: super::HoleLimits,
    }
    #[operation(slot = 1, output = (), error = HoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Seal {
        pub token: super::PieToken,
    }
    #[operation(slot = 2, output = (), error = HoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Push {
        pub token: super::PieToken,
        pub msg: super::VirtAddr,
        pub len: usize,
    }
    #[operation(slot = 3, output = super::PullOutcome, error = HoleFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Pull {
        pub token: super::PieToken,
        pub buf: super::VirtAddr,
        pub max: usize,
        pub oversize: super::Oversize,
    }
    #[operation(slot = 4, output = (usize, super::TaskId, usize), error = HoleFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Peek {
        pub token: super::PieToken,
    }
    #[operation(slot = 5, output = bool, error = HoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Wait {
        pub token: super::PieToken,
        pub condition: super::HoleCondition,
        pub millis: super::Wait,
    }
    #[operation(slot = 6, output = (), error = HoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Ring {
        pub token: super::PieToken,
    }
    #[operation(slot = 7, output = (), error = HoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Hush {
        pub token: super::PieToken,
    }
}
pub use operations::{HoleCall, HoleFail};
