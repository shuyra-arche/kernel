//! PCI configuration and endpoint admission.

mod ecam;
mod endpoint;

pub use ecam::Ecam;
pub(crate) use endpoint::Endpoint;
