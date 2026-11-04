#![no_std]
//! Provisional kernel-std facade for the experimental runtime split.
//!
//! This crate owns no ABI definitions. It re-exports the canonical `kernel-api`
//! env/wire vocabulary and the three mechanism packages imported under
//! `std/{execution,resource,ipc}`. The opt-in runtime registrations live in
//! `kernel-std-rt`, not here.

pub use env;
pub use execution;
pub use ipc;
pub use resource;
pub use wire;
