//! Per-page publication receipts and the contiguous runs used to install them.

mod receipt;
mod runs;

pub(crate) use receipt::Publication;
pub(in crate::basis::nest) use runs::CellRuns;
