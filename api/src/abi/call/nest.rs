//! Address-space operations. External Nests are admitted by trusted boot and
//! delegated through Pend; no Unit operation accepts a hardware requester.
use crate::{address::VirtAddr, model::identity::PendId};
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
        NotAligned = -5,
        AlreadyMapped = -6,
        NoRegion = -7,
    }
    /// Create a private Internal Nest using None(NEST) authority.
    #[operation(slot = 0, output = super::PendId, error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Create {
        pub license: super::PendId,
    }
    #[operation(slot = 1, output = (), error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub token: super::PendId,
        pub out: super::VirtAddr,
    }
    // Slots 2/3 (Attach/Detach) are retired. Pend delegation does not bind hardware.
    /// Close the entire Nest. Busy retains cleanup until hardware has settled;
    /// retrying Delete completes deletion. Revoking a child Pend is not Delete.
    #[operation(slot = 4, output = (), error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Delete {
        pub token: super::PendId,
    }
    /// Atomically reserve vacant positions and return a fresh mapping-scoped root Pend.
    #[operation(slot = 5, output = (super::PendId, super::VirtAddr), error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Reserve {
        pub token: super::PendId,
        pub request: super::VirtAddr,
    }
    /// Retire a mapping's complete original reservation; Busy retains cleanup.
    #[operation(slot = 6, output = (), error = NestFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Release {
        pub token: super::PendId,
    }
}
pub use operations::{NestCall, NestFail};
