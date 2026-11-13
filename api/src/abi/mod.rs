//! Operation contracts and their concrete register representation.
pub mod call;
pub mod error;
pub mod layout;
/// Inactive next-memory-ABI descriptors; not accepted by current dispatch.
pub mod memory;
pub mod operation;
pub mod register;
pub mod reply;

/// All callers and descriptors must use this version; other versions are rejected.
pub const VERSION: u16 = 10;
/// The call number packs `version[63:48]`, `domain[47:32]` and `operation[31:0]`.
pub const PREFIX: usize = (VERSION as usize) << 48;
