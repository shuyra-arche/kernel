//! Opaque owner failure for the Lineage fixture; no resource or wire behavior
//! is simulated here. The suite tests real lineage checks and owner failure propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessError {
    Denied,
    OutOfMemory,
    Busy,
}
