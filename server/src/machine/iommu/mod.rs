//! Platform-specific register access. Protocol and Cell obligations live in Nest.
pub(crate) mod controllers;
pub(crate) mod interrupt;
mod mmio;
pub(crate) mod retirement;
pub(crate) use mmio::Mmio;
