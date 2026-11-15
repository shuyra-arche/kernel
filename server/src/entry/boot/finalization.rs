//! Required finalization effects. Device/backend proofs precede the barrier;
//! post-barrier housekeeping must need no Task, interrupt or remote-Hart work.
use crate::basis::room::shutdown::{Converged, Observation, Unsettled};
pub(super) fn prepare() -> Result<(), Unsettled> {
    super::devices::prepare_iommu_shutdown()
}
pub(super) fn finish(_harts: Converged) {
    crate::basis::room::scheduler::rip();
    crate::metal::allocator::block::flush();
}
/// Optional, bounded read-only diagnostics; called outside COMMIT after required
/// housekeeping. No authority, reclaim decision or completion value is returned.
pub(super) fn report(_snapshot: Observation) {
    if let Some(route) = crate::basis::room::interrupt::external() {
        route.report();
    }
    super::devices::report_iommu_shutdown();
    crate::entry::report::shutdown::report();
}

pub(super) fn unsettled(_snapshot: Observation, reason: Unsettled) {
    crate::putln!(
        "shutdown: hardware {:?}; resources retained; normal reset withheld",
        reason
    );
}
