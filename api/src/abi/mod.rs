//! Operation contracts and their concrete register representation.
pub mod call;
pub mod error;
pub mod layout;
pub mod operation;
pub mod register;
pub mod reply;

/// Version 6 removes Port and runtime external binding. Older calls are rejected.
pub const VERSION: u16 = 6;
/// The call number packs `version[63:48]`, `domain[47:32]` and `operation[31:0]`.
pub const PREFIX: usize = (VERSION as usize) << 48;
