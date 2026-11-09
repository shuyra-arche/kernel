//! External source facts. Nest.Attach consumes Port ATTACH authority; this
//! interface cannot create a source from a caller-supplied hardware number.
use crate::{address::VirtAddr, model::identity::PendId};
pub type PortResult<T> = Result<T, erra::Error<PortFail>>;
#[kernel_api_macros::envcall(class = 14, call = PortCall)]
pub mod operations {
    #[error_domain]
    pub enum PortFail {
        Denied = -1,
        Dead = -2,
        #[busy]
        Busy = -3,
        OutOfMemory = -4,
    }
    #[operation(slot = 0, output = (), error = PortFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Inspect {
        pub token: super::PendId,
        pub out: super::VirtAddr,
    }
}
pub use operations::{PortCall, PortFail};
