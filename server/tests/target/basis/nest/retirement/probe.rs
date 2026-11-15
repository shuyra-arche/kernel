//! Observe real source visits, not synthetic completion or retirement evidence.
use core::sync::atomic::{AtomicUsize, Ordering};
static TURNS: AtomicUsize = AtomicUsize::new(0);
static TASK_RECORDS: AtomicUsize = AtomicUsize::new(0);
static FALLBACK_RECORDS: AtomicUsize = AtomicUsize::new(0);
const CANCEL: bool = false;

pub(crate) fn visited() {
    let id = {
        let _commit = crate::basis::commit();
        crate::basis::room::scheduler::slot::current()
            .running_task()
            .map(|task| task.ident.id.get())
    };
    if id.is_some_and(|id| super::WORK.identifies(id)) {
        TASK_RECORDS.fetch_add(1, Ordering::Relaxed);
    } else if !super::WORK.bound() {
        FALLBACK_RECORDS.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn record() {
    let turn = TURNS.fetch_add(1, Ordering::Relaxed) + 1;
    if CANCEL && turn == 8 {
        // Deliberately skip WORK.close(). No critical turn or resource borrow
        // survives this boundary. Reaped detection must recover the service.
        crate::metal::switcher::internal::exit();
    }
}

pub(crate) fn report() {
    let turns = TURNS.load(Ordering::Relaxed);
    let task = TASK_RECORDS.load(Ordering::Relaxed);
    let fallback = FALLBACK_RECORDS.load(Ordering::Relaxed);
    assert!(turns >= if CANCEL { 8 } else { 2 });
    assert!(task > 0, "resident never advanced a real source record");
    assert!(!CANCEL || fallback > 0, "no fallback after cancellation");
    assert!(!super::WORK.bound());
    assert!(!crate::basis::cell::change::pending());
    assert!(!crate::basis::nest::index::source::pending());
    let (pushed, reaped) = crate::basis::unit::lifecycle::tally::counts();
    assert_eq!(pushed, reaped);
    crate::putln!(
        "PASS retirement turns={turns} task-records={task} fallback-records={fallback} cancel={} pending=0 retained-source=0",
        usize::from(CANCEL)
    );
}
