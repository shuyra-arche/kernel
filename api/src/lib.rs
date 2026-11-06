#![no_std]
//! `kernel-api` umbrella in the shared workspace.
//!
//! The canonical definitions now live in the owned packages under `env/` and
//! `wire/`. This crate only re-exports those two packages; it defines no ABI,
//! codec, wrapper, forwarding function, or trait of its own.
//!
//! The two `wire` module scopes remain distinct:
//!
//! - [`env::wire`] is the register/ABI wire vocabulary owned by `kernel-api-env`;
//! - [`wire`] is the standalone byte/protocol codec owned by `kernel-api-wire`.
//!
//! The module scopes differ, but [`env::wire::Field`]/[`env::wire::Span`] and
//! [`wire::Field`]/[`wire::Span`] intentionally name the same canonical traits
//! from `kernel-api-wire`, re-exported through the env vocabulary. Keeping the
//! explicit crate re-exports makes the two paths unambiguous without creating
//! duplicate trait identities.

#![forbid(unsafe_code)]

pub use kernel_api_env as env;
pub use kernel_api_wire as wire;
