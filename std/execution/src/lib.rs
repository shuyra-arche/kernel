#![no_std]
//! User execution primitives for tasks, memory and program startup.
//!
//! The opt-in `_start`/panic/global-allocator registrations are intentionally
//! absent from this package. The runtime package (`kernel-std-rt`) owns those
//! symbols and calls [`bootstrap_tls`] explicitly from its startup sequence.

extern crate alloc;

pub mod lock;
pub mod memory;
pub mod room;
pub mod unit;

/// Allocate and install this task's initial thread-local storage block.
///
/// # Safety
///
/// This is the explicit seam used by `kernel-std-rt` from `tls_bootstrap`.
/// It must be called exactly once for a program task, before any Rust code
/// observes the TLS block, and only while `_start` owns the initial stack.
#[cfg(target_arch = "riscv64")]
pub unsafe fn bootstrap_tls() {
    unsafe { unit::bootstrap() }
}
