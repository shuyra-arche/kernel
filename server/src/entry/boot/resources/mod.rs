//! Boot resource grants, staging and the entry task's ledger page.

mod authority;
mod data;
pub(crate) mod ledger;
mod registry;

pub(crate) use authority::{
    CALL_ENTRIES, CONTROL_ENTRIES, grant_calls, grant_controls, register_space,
};
pub(super) use data::snapshot;
pub(crate) use registry::Registry;
