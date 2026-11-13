//! Operation contracts and their concrete register representation.
pub mod call;
pub mod error;
pub mod layout;
/// Memory descriptors selected by the coordinated `memory-v11` image feature.
pub mod memory;
pub mod operation;
pub mod register;
pub mod reply;

/// All callers and descriptors must use this version; other versions are rejected.
#[cfg(not(feature = "memory-v11"))]
pub const VERSION: u16 = 10;
/// Coordinated native image: never accepts v10 calls or boot descriptors.
#[cfg(feature = "memory-v11")]
pub const VERSION: u16 = 11;
/// The call number packs `version[63:48]`, `domain[47:32]` and `operation[31:0]`.
pub const PREFIX: usize = (VERSION as usize) << 48;
