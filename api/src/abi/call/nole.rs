//! Notifications without indexed signal bits.
use crate::model::identity::PieToken;
use crate::model::wait::Wait;

pub type NoleResult<T> = Result<T, erra::Error<NoleFail>>;

#[kernel_api_macros::envcall(class = 11, call = NoleCall)]
pub mod operations {
    #[error_domain]
    pub enum NoleFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
    }
    #[operation(slot = 0, output = super::PieToken, error = NoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Unseal;
    #[operation(slot = 1, output = (), error = NoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Seal {
        pub token: super::PieToken,
    }
    #[operation(slot = 2, output = bool, error = NoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Wait {
        pub token: super::PieToken,
        pub millis: super::Wait,
    }
    #[operation(slot = 3, output = (), error = NoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Ring {
        pub token: super::PieToken,
    }
    #[operation(slot = 4, output = (), error = NoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Hush {
        pub token: super::PieToken,
    }
}
pub use operations::{NoleCall, NoleFail};
