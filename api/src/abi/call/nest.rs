use crate::{
    address::VirtAddr,
    model::{identity::PendId, nest::NestKind},
};
pub type NestResult<T> = Result<T, erra::Error<NestFail>>;
#[kernel_api_macros::envcall(class = 13, call = NestCall)]
pub mod operations {
    #[error_domain]
    pub enum NestFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OutOfMemory = -4,
    }
    #[operation(slot = 0, output = super::PendId, error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create {
        pub license: super::PendId,
        pub kind: super::NestKind,
    }
    #[operation(slot = 1, output = (), error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub token: super::PendId,
        pub out: super::VirtAddr,
    }
    #[operation(slot = 2, output = (), error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Attach {
        pub token: super::PendId,
        pub port: super::PendId,
    }
    #[operation(slot = 3, output = (), error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Detach {
        pub token: super::PendId,
    }
    #[operation(slot = 4, output = (), error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub token: super::PendId,
    }
}
pub use operations::{NestCall, NestFail};
