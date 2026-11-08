//! Resource and execution mechanisms; state transitions are explicit.

pub(crate) mod cell;
pub(crate) mod flat;
pub(crate) mod nest;
pub(crate) mod zone;

pub(crate) mod pend;
pub(crate) mod sign;
pub(crate) mod view;

pub(crate) mod room;
pub(crate) mod unit;

mod commit;

#[cfg(debug_assertions)]
pub(crate) use commit::assert_held;
#[cfg(debug_assertions)]
pub(crate) use commit::assert_unlocked;
pub(crate) use commit::commit;
