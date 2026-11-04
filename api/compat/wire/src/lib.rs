#![no_std]
//! Compatibility facade named `wire`.
//!
//! This crate is a re-export-only adapter for the isolated PR2 identity and
//! macro fixtures. It depends directly on the owning package, never on the
//! umbrella, so the dev-dependency graph cannot cycle through `kernel-api`.

pub use kernel_api_wire::*;
