//! Production typed locks, FIFO atomics and bounded diagnostics on a host IRQ boundary.
mod provider;
mod support;
pub(crate) use support::{basis, metal};
mod ordering;
mod ui;
