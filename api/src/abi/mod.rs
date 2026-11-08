//! Operation contracts and their concrete register representation.
pub mod call;
pub mod error;
pub mod layout;
pub mod operation;
pub mod register;
pub mod reply;

/// Incompatible register and descriptor contract. Version zero is retired.
pub const VERSION: u16 = 2;
/// The call number packs `version[63:48]`, `domain[47:32]` and `operation[31:0]`.
pub const PREFIX: usize = (VERSION as usize) << 48;
