//! Identity allocation policies. Allocation never publishes an object or proves
//! that hardware has stopped using it; those obligations belong to the caller.
mod generation;
mod recycle;
mod serial;

pub(crate) use generation::Generation;
pub(crate) use recycle::Recycle;
pub(crate) use serial::Serial;
