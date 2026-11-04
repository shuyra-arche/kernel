#![no_std]
//! Isolated `kernel-api` candidate.
//!
//! This package is deliberately only a transparent facade over the canonical
//! `env` and `wire` crates at the pinned revisions. It defines no types,
//! wrappers, trait implementations, ABI encodings, or codecs of its own.
//!
//! Two distinct `wire` names remain distinct:
//!
//! - [`env::wire`] is the register/ABI wire vocabulary owned by `env`;
//! - [`wire`] is the standalone byte/protocol codec crate.
//!
//! They are not interchangeable. Keeping them as explicit crate re-exports
//! makes `kernel_api::env::wire` and `kernel_api::wire` unambiguously different
//! paths.

#![forbid(unsafe_code)]

pub use env;
pub use wire;
