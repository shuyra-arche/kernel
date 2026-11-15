//! Identity allocation policies. Allocation never publishes an object or proves
//! that hardware has stopped using it; those obligations belong to the caller.
mod arena;
mod generation;
mod recycle;
mod registry;
mod serial;

pub(crate) use arena::{Arena, Key};
pub(crate) use generation::Generation;
pub(crate) use recycle::Recycle;
pub(crate) use registry::Registry;
pub(crate) use serial::Serial;

/// Shared identity stream for Clone and exported Flat/Share observations.
/// Allocation does not construct a View or grant resource access.
pub(crate) fn view() -> Option<usize> {
    static NEXT: Serial = Serial::new(1, isize::MAX as usize);
    NEXT.allocate()
}
