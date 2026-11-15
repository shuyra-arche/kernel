//! Resident Kernel Team Tasks, grouped by independently scheduled responsibility.
//! Boot constructs and starts each Task explicitly; this module is not a dispatcher.
//! Keep entry, startup, wake sources and bounded loop together in each child.
//! Authoritative resource state and retirement evidence remain with basis owners.
pub(crate) mod observe;
