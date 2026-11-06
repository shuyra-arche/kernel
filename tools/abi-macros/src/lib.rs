//! Kernel operation declarations. Generated paths refer to the canonical API.
//!
//! This package owns neither generic byte codecs nor service protocols.

mod envcall;
mod fail;

use proc_macro::TokenStream;

/// Declare operations and their error domains in an inline module.
/// `#[operation(slot = N, output = T, error = E)]` describes a returning call;
/// use `infallible` instead of `error = E`, or `nonreturning` without an output.
/// `reply = 3` selects three result registers (the default is two).
/// `#[error_domain]` declares Kernel's consecutive negative failure codes.
/// No Client functions, transport, or runtime state are generated.
#[proc_macro_attribute]
pub fn envcall(attr: TokenStream, item: TokenStream) -> TokenStream {
    envcall::expand(attr.into(), item.into()).into()
}
