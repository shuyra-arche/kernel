//! Share Views and Pend-authorized CPU mappings.
use crate::address::VirtAddr;
use crate::model::cell::FlatReference;
use crate::model::identity::PendId;

pub type ShareResult<T> = Result<T, erra::Error<ShareFail>>;

#[kernel_api_macros::envcall(class = 10, call = ShareCall)]
pub mod operations {
    #[error_domain]
    pub enum ShareFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OoM = -4,
        HandedOver = -5,
        NotAligned = -6,
    }
    #[operation(slot = 0, output = super::PendId, error = ShareFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create {
        pub reference: super::FlatReference,
        pub modifiable: crate::model::modifiable::Modifiable,
    }
    #[operation(slot = 1, output = (), error = ShareFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub token: super::PendId,
    }
    #[operation(slot = 2, output = (super::VirtAddr, usize), error = ShareFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Open {
        pub token: super::PendId,
    }
    #[operation(slot = 3, output = (), error = ShareFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Shut {
        pub token: super::PendId,
    }
}
pub use operations::ShareCall;
pub use operations::ShareFail;
