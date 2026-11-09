//! Trusted Rust boot contracts, separate from the syscall wire ABI.
//! Memory reservations and physical resource custody are provider-independent.
//! CPU, timer and controller admission belongs to the selected architecture;
//! currently only `riscv` defines a complete boot input. No generic ISA enum or
//! unimplemented provider contract is implied by the common physical facts.
pub mod memory;
mod region;
pub mod resources;
pub mod riscv;

pub use memory::{Memory, MemoryError};
pub use region::Region;
pub use resources::Resources;
