//! Bounded message queues and counted notifications.
use crate::address::VirtAddr;
use crate::model::clone_view::PullOutcome;
use crate::model::identity::Mark;
use crate::model::identity::PendId;
use crate::model::identity::TaskId;

use crate::model::clone_view::CloneLimits;
pub type CloneResult<T> = Result<T, erra::Error<CloneFail>>;

#[kernel_api_macros::envcall(class = 5, call = CloneCall)]
pub mod operations {
    #[error_domain]
    pub enum CloneFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OutOfMemory = -4,
        HandedOver = -5,
        Gone = -6,
    }
    #[operation(slot = 0, output = super::PendId, error = CloneFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create {
        pub mark: super::Mark,
        pub limits: super::CloneLimits,
    }
    #[operation(slot = 1, output = (), error = CloneFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub token: super::PendId,
    }
    #[operation(slot = 2, output = (), error = CloneFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Push {
        pub token: super::PendId,
        pub msg: super::VirtAddr,
        pub len: usize,
    }
    /// Receive the existing copied Cells as a read-only payload Pend. The length
    /// limit bounds admission; failure keeps the message queued. Slot 3 (copy-out)
    /// is retired so old callers cannot mistake a Pend for a byte count.
    #[operation(slot = 8, output = super::PullOutcome, error = CloneFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Pull {
        pub token: super::PendId,
        pub max_len: usize,
    }
    #[operation(slot = 4, output = (usize, super::TaskId, usize), error = CloneFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Peek {
        pub token: super::PendId,
    }
}
pub use operations::{CloneCall, CloneFail};

/// Clone Peek returns the queued length, sender and queue depth.
impl crate::abi::reply::FromTriple for (usize, TaskId, usize) {
    fn from_triple(v0: usize, v1: usize, v2: usize) -> Self {
        (v0, TaskId(v1), v2)
    }
}
