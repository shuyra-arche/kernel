#[path = "../../../../src/basis/nest/index/source.rs"]
pub(crate) mod mechanism;
use super::{Arc, FlatId, FlatRange, RangeBinding, Span, allocation, basis::sign};
use std::sync::atomic::{AtomicBool, Ordering};
pub(super) static AGAIN: AtomicBool = AtomicBool::new(false);

fn drain() -> usize {
    let mut turns = 0;
    while mechanism::pending() {
        allocation::fail_after(0, || super::host_lock::single(mechanism::advance));
        turns += 1;
        assert!(turns < 4096, "pass did not settle");
    }
    turns
}

#[test]
fn bounded_pass_retains_late_completion_and_never_needs_allocation() {
    let flat = FlatRange(FlatId::new(1).unwrap());
    let spans: Vec<_> = (1..=37).map(|id| Arc::new(Span(id))).collect();
    let binding = Arc::new(RangeBinding);
    for span in &spans {
        mechanism::register(&flat, span, Some(&binding)).unwrap();
    }
    // Duplicate discovery cannot multiply wakeup work.
    mechanism::register(&flat, &spans[0], Some(&binding)).unwrap();
    sign::prepare();
    mechanism::request();
    assert!(allocation::fail_after(0, mechanism::advance));
    let initial = sign::take();
    assert_eq!(initial.len(), 4);
    sign::prepare();
    // Another completion after the first nodes were visited must revisit them.
    // Delivery also requests once from outside the index lock.
    mechanism::request();
    AGAIN.store(true, Ordering::Relaxed);
    assert!(drain() >= 9);
    let rest = sign::take();
    for id in 1..=37 {
        assert!(rest.contains(&id));
    }
    for id in initial {
        assert!(rest.contains(&id));
    }

    // A later prune request cannot downgrade a wake queued behind a prune pass.
    sign::prepare();
    mechanism::forget();
    mechanism::advance();
    mechanism::request();
    mechanism::forget();
    drain();
    let notified = sign::take();
    assert!(spans.iter().all(|span| notified.contains(&span.0)));

    // Pruning does not signal live waiters, and an expired binding does not
    // keep the Span or any authoritative resource alive.
    sign::prepare();
    drop(binding);
    mechanism::forget();
    assert!(drain() >= 9);
    assert!(sign::take().is_empty());
    assert!(spans.iter().all(|span| Arc::strong_count(span) == 1));
    drop(spans);
    drain();

    // New registrations during an active pass cannot starve its old tail.
    let old: Vec<_> = (100..109).map(|id| Arc::new(Span(id))).collect();
    for span in &old {
        mechanism::register(&flat, span, None).unwrap();
    }
    sign::prepare();
    mechanism::request();
    allocation::fail_after(0, mechanism::advance);
    let mut newer = Vec::new();
    for id in 200..208 {
        let span = Arc::new(Span(id));
        mechanism::register(&flat, &span, None).unwrap();
        newer.push(span);
        if !mechanism::advance() {
            break;
        }
    }
    let reached = sign::take();
    assert!(old.iter().all(|span| reached.contains(&span.0)));
    assert!(!mechanism::pending());
    drop((old, newer));
    drain();
}
