//! Shared identities, resources, modifiability and lifecycle values.
//! These values carry no allocation, cleanup, scheduling or execution policy.

pub mod cell;
pub mod clock;
pub mod exit;
pub mod identity;
pub mod modifiable;
pub mod pend;
pub mod sign;
pub mod unit;
pub mod view;
pub mod wait;

pub use view::clone as clone_view;
