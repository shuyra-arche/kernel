//! Platform-specific register access. Protocol and Cell obligations live in Nest.
mod mmio;
pub(crate) mod retirement;
pub(crate) use mmio::Mmio;
