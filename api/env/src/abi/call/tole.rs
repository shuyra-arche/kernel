use crate::{PieToken, Wait};

pub use super::tole_await_reply::ToleAwaitReply;
pub use super::tole_event_source::ToleEventSource;
pub type ToleResult<T> = Result<T, erra::Error<ToleFail>>;

#[kernel_api_macros::envcall(class = 12, call = ToleCall)]
pub mod operations {
    #[error_domain]
    pub enum ToleFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
    }
    #[error_domain]
    pub enum ToleEventFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
        Gone = -6,
    }
    #[operation(slot = 0, output = super::PieToken, error = ToleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Unseal {
        pub shared: bool,
    }
    #[operation(slot = 1, output = (), error = ToleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Seal {
        pub token: super::PieToken,
    }
    #[operation(slot = 2, output = (), error = ToleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Attach {
        pub tole: super::PieToken,
        pub source: super::ToleEventSource,
    }
    #[operation(slot = 3, output = (), error = ToleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Detach {
        pub tole: super::PieToken,
        pub source: super::ToleEventSource,
    }
    #[operation(slot = 4, output = super::ToleAwaitReply, error = ToleFail, reply = 3)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Await {
        pub tole: super::PieToken,
        pub millis: super::Wait,
    }
}
pub use operations::{ToleCall, ToleEventFail, ToleFail};
