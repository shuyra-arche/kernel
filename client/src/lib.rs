#![no_std]
//! Typed Kernel calls and register transport. Import contracts from `kernel_api`.

pub mod call;
pub mod invoke;
/// Next-memory ABI client. Requires a coordinated v11 provider; v10 rejects it.
pub mod memory;
pub mod transport;
