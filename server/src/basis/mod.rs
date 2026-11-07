//! Resource and execution mechanisms; state transitions are explicit.
pub(crate) mod cell;
mod commit;
pub(crate) mod flat;
pub(crate) mod nest;
pub(crate) mod pend;
pub(crate) mod room;
pub(crate) mod sign;
pub(crate) mod unit;
pub(crate) mod view;
#[cfg(debug_assertions)]
pub(crate) use commit::assert_held;
#[cfg(debug_assertions)]
pub(crate) use commit::assert_unlocked;
pub(crate) use commit::commit;

pub(crate) mod zone;
