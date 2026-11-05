//! Lifecycle hooks for a runtime adapter. These install no entry or panic handler.

pub use crate::unit::tls::{Cleanup, Tls, initialize as initialize_tls, retire as retire_tls};
