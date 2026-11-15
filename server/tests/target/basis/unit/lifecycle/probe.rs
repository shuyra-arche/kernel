//! Composed into the real resident Task, never production instrumentation.
use core::sync::atomic::{AtomicUsize, Ordering};
static TURNS: AtomicUsize = AtomicUsize::new(0);
const CANCEL: bool = false;
pub(crate) fn record() {
    let turn = TURNS.fetch_add(1, Ordering::Relaxed) + 1;
    if CANCEL && turn == 8 {
        // Intentionally skip the normal unregister epilogue. The source must
        // detect a dead service and hand its retained work back to fallback.
        super::super::exit();
    }
}
pub(crate) fn report() {
    let turns = TURNS.load(Ordering::Relaxed);
    assert!(turns >= if CANCEL { 8 } else { 2 }, "resident never resumed its loop");
    assert!(!crate::basis::unit::lifecycle::WORK.bound());
    let (pushed, reaped) = crate::basis::unit::lifecycle::tally::counts();
    assert_eq!(pushed, reaped, "cleanup Task depends on its own reap");
    crate::putln!("PASS lifecycle turns={} cancel={} retained-source=0", turns, usize::from(CANCEL));
}
