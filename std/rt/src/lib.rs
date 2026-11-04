#![no_std]
//! Experimental opt-in runtime registrations for `kernel-std` programs.
//!
//! # Feature contract
//!
//! - `entry`: emits `_start`, `save_args`, `tls_bootstrap`, and the hygienic
//!   [`entry!`] macro that defines `clean_ret`.
//! - `panic-handler`: emits `#[panic_handler]`.
//! - `global-allocator`: registers the single `kernel-std` heap as
//!   `#[global_allocator]`.
//!
//! No feature is enabled by default. RV program registration is therefore
//! explicit and selected by the binary.

pub mod boot;

pub use boot::{Exit, Report};

/// Define the `clean_ret` symbol for a program entry point.
///
/// The supplied path must be an `fn() -> R` where `R: $crate::boot::Exit`.
/// `_start` (enabled by this same `entry` feature) calls `save_args`, then
/// `tls_bootstrap`, then `clean_ret`.
#[cfg(feature = "entry")]
#[macro_export]
macro_rules! entry {
    ($path:path) => {
        #[unsafe(no_mangle)]
        extern "C" fn clean_ret() -> ! {
            $crate::boot::entry($path)
        }
    };
}

#[cfg(all(target_arch = "riscv64", feature = "global-allocator"))]
#[global_allocator]
static PROGRAM_HEAP: kernel_std::execution::memory::HeapAllocator =
    kernel_std::execution::memory::HeapAllocator;
