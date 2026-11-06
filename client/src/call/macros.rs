// Client owns names and execution. Request types determine their result types.
macro_rules! typed_calls {
    ($($module:ident [$($imports:tt)*] {
        $($mode:ident fn $name:ident($($arg:ident: $ty:ty),*) -> $output:ty
            = $request:expr;)*
    })*) => {
        $(pub mod $module {
            typed_calls!(@imports $($imports)*);
            use kernel_api::abi::call::$module::operations as op;
            $(#[inline]
            pub fn $name($($arg: $ty),*) -> $output {
                typed_calls!(@invoke $mode $request)
            })*
        })*
    };
    (@imports) => {};
    (@imports $($imports:tt)+) => { use kernel_api::{$($imports)+}; };
    (@invoke terminal $request:expr) => {
        unsafe { super::invoke_terminal($request) }
    };
    (@invoke $mode:ident $request:expr) => {
        {
            let result = unsafe { super::invoke_call($request) };
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
