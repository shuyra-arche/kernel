//! Typed entry points over the canonical register-call contracts.

// Client owns names and execution. Request types determine their result types.
macro_rules! typed_calls {
    ($($mode:ident fn $name:ident($($arg:ident: $ty:ty),*) -> $output:ty
        = $request:expr;)*) => {
        $(#[inline]
        pub fn $name($($arg: $ty),*) -> $output {
            typed_calls!(@invoke $mode $request)
        })*
    };
    (@invoke terminal $request:expr) => {
        unsafe { crate::invoke::invoke_terminal($request) }
    };
    (@invoke $mode:ident $request:expr) => {
        {
            let result = unsafe { crate::invoke::invoke($request) };
            typed_calls!(@result $mode result)
        }
    };
    (@result infallible $result:ident) => {
        match $result {
            Ok(value) => value,
            Err(never) => match never {},
        }
    };
    (@result fallible $result:ident) => { $result };
}

pub mod cell;
pub mod debug;
pub mod pend;
pub mod room;
pub mod sign;
pub mod unit;
mod view;

pub use room::deadline_after;
pub use view::{clone as clone_view, share as share_view};
