//! Real finalization composition: required effects remain when observers are absent.
use core::sync::atomic::{AtomicUsize, Ordering};
const QUIET: bool = false;
static PREPARED: AtomicUsize = AtomicUsize::new(0);
pub(crate) fn prepared() {
    assert_eq!(crate::basis::room::shutdown::barrier().0, 0, "barrier preceded hardware preparation");
    assert_eq!(PREPARED.fetch_add(1, Ordering::Relaxed), 0);
}
pub(crate) fn finished() {
    let (arrived, total) = crate::basis::room::shutdown::barrier();
    assert_eq!(arrived, total);
    assert_eq!(PREPARED.load(Ordering::Relaxed), 1);
    let (pushed, reaped) = crate::basis::unit::lifecycle::tally::counts();
    assert_eq!(pushed, reaped);
    if QUIET { crate::metal::mmu::internal::completion::probe::report_migrations(); }
    crate::putln!("PASS shutdown required=1 prepared=1 converged=1");
}
