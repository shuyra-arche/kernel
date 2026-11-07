//! Shared identities, resources, modifiability and lifecycle values.
//! These values carry no allocation, cleanup, scheduling or execution policy.

pub mod clone_view;
pub mod exit;
pub mod identity;
pub mod memory;
pub mod modifiable;
pub mod pend;
pub mod sign;
pub mod unit;
pub mod view;
pub mod wait;
