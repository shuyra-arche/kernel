// Client owns names and execution. Request types determine their result types.
macro_rules! typed_calls {
    ($($module:ident {
        $($mode:ident fn $name:ident($($arg:ident: $ty:ty),*) -> $output:ty
            = $request:expr;)*
    })*) => {
        $(pub mod $module {
            #[allow(unused_imports)]
            use env::*;
            use env::abi::call::$module::operations as op;
            $(#[inline]
            pub fn $name($($arg: $ty),*) -> $output {
                let result = unsafe { super::invoke_call($request) };
                typed_calls!(@result $mode result)
            })*
        })*
    };
    (@result infallible $result:ident) => {
        match $result {
            Ok(value) => value,
            Err(never) => match never {},
        }
    };
    (@result fallible $result:ident) => { $result };
}
