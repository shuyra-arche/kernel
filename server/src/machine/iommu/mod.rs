//! Platform-specific register access. Protocol and Cell obligations live in Nest.
mod mmio;
pub(crate) use mmio::Mmio;
