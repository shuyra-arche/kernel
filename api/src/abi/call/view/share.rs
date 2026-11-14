//! Share Views and Pend-authorized CPU mappings.
use crate::address::VirtAddr;
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
        OutOfMemory = -4,
        HandedOver = -5,
        NotAligned = -6,
        /// This source has no qualified CPU activity provider or supported geometry.
        Unsupported = -7,
    }
    #[operation(slot = 1, output = (), error = ShareFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub token: super::PendId,
    }
    /// Native v11 source derivation. `request` points to one AccordRequest;
    /// A live Flat/Share source derives ordinary Share access.
    /// A Reservation source with INSPECT|SAMPLE derives one readonly activity
    /// page in the requested Pages window and a companion Sign in the recipient
    /// ledger. The two Pends form (Sign, Share View); no new call is needed.
    /// The creation license and source authority are checked in one commit.
    #[operation(slot = 4, output = super::PendId, error = ShareFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Derive {
        pub license: super::PendId,
        pub request: super::VirtAddr,
    }
}
pub use operations::ShareCall;
pub use operations::ShareFail;

/// Share Open returns the view's address and length.
impl crate::abi::reply::FromPair for (VirtAddr, usize) {
    fn from_pair(v0: usize, v1: usize) -> Self {
        (VirtAddr(v0), v1)
    }
}
