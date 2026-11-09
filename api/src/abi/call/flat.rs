use crate::{
    address::VirtAddr,
    model::{cell::SlotId, identity::PendId},
};
pub type FlatResult<T> = Result<T, erra::Error<FlatFail>>;
#[kernel_api_macros::envcall(class=12,call=FlatCall)]
pub mod operations {
    #[error_domain]
    pub enum FlatFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OutOfMemory = -4,
        NotAligned = -5,
        HandedOver = -6,
    }
    #[operation(slot=0,output=super::PendId,error=FlatFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create {
        pub license: super::PendId,
        pub size: usize,
        pub align: usize,
    }
    #[operation(slot=1,output=(),error=FlatFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub token: super::PendId,
        pub out: super::VirtAddr,
    }
    #[operation(slot=2,output=super::SlotId,error=FlatFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Slice {
        pub token: super::PendId,
        pub offset: usize,
        pub count: usize,
    }
    #[operation(slot=3,output=(),error=FlatFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub token: super::PendId,
    }
}
pub use operations::{FlatCall, FlatFail};
