//! Per-page publication receipts and the contiguous runs used to install them.

mod receipt;
mod runs;

pub(crate) use receipt::{Phase, Publication};
pub(in crate::basis::nest) use runs::CellRuns;

mod error;
mod identity;
pub(crate) use error::Error;
