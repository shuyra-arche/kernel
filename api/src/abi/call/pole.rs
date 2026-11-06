//! Shared-memory mappings and indexed notifications.
use crate::model::identity::{PieToken, VirtAddr};
use crate::model::memory::FlatReference;
use crate::model::wait::Wait;

use crate::model::pole::{Bit, PoleCondition};
pub type PoleResult<T> = Result<T, erra::Error<PoleFail>>;

#[kernel_api_macros::envcall(class = 10, call = PoleCall)]
pub mod operations {
    #[error_domain]
    pub enum PoleFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
        NotAligned = -6,
    }
    #[operation(slot = 0, output = super::PieToken, error = PoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Unseal {
        pub reference: super::FlatReference,
        pub shared: bool,
    }
    #[operation(slot = 1, output = (), error = PoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Seal {
        pub token: super::PieToken,
    }
    #[operation(slot = 2, output = (super::VirtAddr, usize), error = PoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Open {
        pub token: super::PieToken,
    }
    #[operation(slot = 3, output = (), error = PoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Shut {
        pub token: super::PieToken,
    }
    #[operation(slot = 4, output = bool, error = PoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Wait {
        pub token: super::PieToken,
        pub condition: super::PoleCondition,
        pub millis: super::Wait,
    }
    #[operation(slot = 5, output = (), error = PoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Ring {
        pub token: super::PieToken,
        pub bit: super::Bit,
    }
    #[operation(slot = 6, output = (), error = PoleFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Hush {
        pub token: super::PieToken,
        pub bit: super::Bit,
    }
}
pub use operations::{PoleCall, PoleFail};
