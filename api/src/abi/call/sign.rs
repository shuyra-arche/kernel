//! One control endpoint supports explicit triggering and authorized source routing.
use crate::abi::reply::SignReply;
use crate::model::identity::TaskId;
use crate::model::sign::SignSource;
use crate::model::sign::{SignId, SignRights};
use crate::model::wait::Wait;

pub type SignResult<T> = Result<T, erra::Error<SignFail>>;

#[kernel_api_macros::envcall(class = 11, call = SignCall)]
pub mod operations {
    #[error_domain]
    pub enum SignFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
    }
    #[error_domain]
    pub enum SignEventFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
        Gone = -6,
    }
    #[operation(slot = 0, output = super::SignId, error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create;
    #[operation(slot = 1, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Close {
        pub token: super::SignId,
    }
    /// Observe a retained direct trigger or a ready source; does not consume it.
    /// A timeout only ends this wait, without cancelling the source operation.
    #[operation(slot = 2, output = super::SignReply, error = SignFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Listen {
        pub token: super::SignId,
        pub millis: super::Wait,
    }
    /// Capacity is one pending direct trigger; a second trigger returns Busy.
    #[operation(slot = 3, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Trigger {
        pub token: super::SignId,
    }
    /// Clear a direct trigger. Routed events retain their source's own semantics.
    #[operation(slot = 4, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Consume {
        pub token: super::SignId,
    }
    /// Register observation of a source already authorized to this Task.
    #[operation(slot = 5, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Route {
        pub token: super::SignId,
        pub source: super::SignSource,
    }
    #[operation(slot = 6, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Unroute {
        pub token: super::SignId,
        pub source: super::SignSource,
    }
    #[operation(slot = 7, output = super::SignId, error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delegate {
        pub token: super::SignId,
        pub target: super::TaskId,
        pub rights: super::SignRights,
    }
    #[operation(slot = 8, output = (), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Release {
        pub token: super::SignId,
    }
    /// Enumerate only this Task's qualified handles; (0, 0) marks the end.
    #[operation(slot = 9, output = (usize, usize), error = SignFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Next {
        pub after: usize,
    }
}
pub use operations::{SignCall, SignEventFail, SignFail};
