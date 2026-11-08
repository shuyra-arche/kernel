//! Protected endpoint configuration and trusted boot apertures.

pub(crate) mod aperture;
mod config;
mod endpoint;

pub use config::Ecam;
pub(crate) use endpoint::Endpoint;
