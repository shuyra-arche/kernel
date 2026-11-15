//! Sampling capability is distinct from data/instruction completion qualification.
use core::sync::atomic::{AtomicBool, Ordering};
static QUALIFIED: AtomicBool = AtomicBool::new(false);
pub(crate) fn available() -> bool {
    QUALIFIED.load(Ordering::Acquire)
}
/// # Safety
/// Single-Hart boot before scheduler publication. Every boot-admitted Hart,
/// including late entrants, must use hardware A/D updates for the active Sv
/// mode, with effective ADUE enabled when required. PTE RAM is coherent for
/// full-width atomic hardware/software updates. Firmware/virtualization must
/// preserve this policy throughout boot. No device activity is qualified.
pub unsafe fn qualify() -> Result<(), crate::metal::generic::memory::Error> {
    use crate::metal::generic::memory::Error;
    if crate::basis::room::scheduler::installed() {
        return Err(Error::Invalid);
    }
    QUALIFIED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| Error::Invalid)
}
