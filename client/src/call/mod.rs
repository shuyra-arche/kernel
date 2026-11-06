//! Client-owned typed entry points over the canonical register-call contracts.

#[macro_use]
mod macros;

use crate::invoke::{invoke as invoke_call, invoke_terminal};

include!("definitions.rs");
