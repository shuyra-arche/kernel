//! Client-owned typed entry points over the canonical register-call contracts.

#[macro_use]
mod macros;

use crate::invoke::{invoke as invoke_call, invoke_terminal};

include!("definitions.rs");

/// Convert once; reusing this deadline across retries does not extend the timeout.
pub fn deadline_after(duration: core::time::Duration) -> kernel_api::model::wait::Wait {
    if duration.is_zero() {
        return kernel_api::model::wait::Wait::Poll;
    }
    let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
    kernel_api::model::wait::Wait::At(room::clock().saturating_add(nanos))
}
