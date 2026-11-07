//! Platform-specific register access. Protocol and Cell obligations live in Nest.
pub(crate) mod controllers;
pub(crate) mod interrupt;
mod mmio;
pub(crate) mod retirement;
pub(crate) use mmio::Mmio;

pub(crate) mod aperture;
mod config;
pub(crate) mod pci;
