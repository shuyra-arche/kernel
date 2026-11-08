//! Boot resource grants, staging and the entry task's ledger page.

mod authority;
pub(crate) mod ledger;
mod registry;

pub(crate) use authority::{
    CALL_ENTRIES, CONTROL_ENTRIES, SIGN_ENTRIES, grant_calls, grant_controls, grant_sign,
};
pub(crate) use registry::Registry;
