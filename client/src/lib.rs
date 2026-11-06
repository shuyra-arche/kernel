#![no_std]
//! Environment calls and reusable task, memory and TLS mechanisms.

extern crate alloc;

#[doc(hidden)]
pub mod __private;
pub mod call;
pub mod ecall;
pub mod invoke;
pub mod lock;
pub mod memory;
pub mod room;
pub mod unit;

pub use env::*;
