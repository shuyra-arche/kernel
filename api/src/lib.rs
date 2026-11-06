#![no_std]
//! Canonical Kernel ABI umbrella.
//!
//! [`env`] owns register calls, handles, failures and concrete Kernel byte
//! layouts. Generic codecs and service protocols belong to their consumers.

#![forbid(unsafe_code)]

pub use kernel_api_env as env;
