//! Translation backends. Internal covers every CPU access, including user mode;
//! external covers non-CPU requesters with distinct retirement evidence.
pub(crate) mod external;
pub(crate) mod internal;
