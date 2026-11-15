//! CPU 翻译身份：ASID 及其失效。

mod asid;
mod error;
mod shootdown;

pub use asid::Asid;

pub(crate) use asid::vacant;
pub(crate) use shootdown::deallocate;
pub(crate) use shootdown::flush;
pub(crate) use shootdown::occupy;
pub(crate) use shootdown::shootdown;
pub(crate) use shootdown::sync_instructions;
pub(crate) use shootdown::vacate;
