extern crate alloc;
mod allocation;
mod arena;
mod generation;
mod registry;
use arena::{Arena, Key};
use generation::Generation;
#[path = "../../src/basis/name/recycle.rs"]
mod recycle;
#[path = "../../src/basis/name/serial.rs"]
mod serial;
use recycle::Recycle;
use serial::Serial;

#[test]
fn exhaustion_is_terminal_even_at_the_machine_limit() {
    for last in [3, isize::MAX as usize, usize::MAX] {
        let ids = Serial::new(last - 1, last);
        assert_eq!(ids.allocate(), Some(last - 1));
        assert_eq!(ids.allocate(), Some(last));
        for _ in 0..10 {
            assert_eq!(ids.allocate(), None);
        }
    }
}

#[test]
fn batch_identity_exhaustion_cannot_consume_a_partial_ticket_set() {
    for last in [3, isize::MAX as usize, usize::MAX] {
        let ids = Serial::new(last - 1, last);
        assert!(ids.reserve(3).is_none());
        assert_eq!(ids.allocate(), Some(last - 1));
        // A former per-piece allocator could prepare the first ticket and fail
        // on the second. Batch failure must preserve the one remaining ID.
        assert!(ids.reserve(2).is_none());
        assert_eq!(ids.reserve(1).unwrap().collect::<Vec<_>>(), [last]);
        assert!(ids.reserve(1).is_none());
        assert!(ids.reserve(0).is_none());
    }
}

#[test]
fn concurrent_ticket_batches_do_not_overlap_or_wrap() {
    let ids = Serial::new(1, 4000);
    let mut all = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let mut result = Vec::new();
                    while let Some(batch) = ids.reserve(2) {
                        result.extend(batch);
                    }
                    result
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    all.sort_unstable();
    assert_eq!(all, (1..=4000).collect::<Vec<_>>());
}
#[test]
fn concurrent_allocations_never_repeat_or_cross_the_domain_limit() {
    let ids = Serial::new(1, 4093);
    let mut all = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let mut result = Vec::new();
                    while let Some(id) = ids.allocate() {
                        result.push(id);
                    }
                    result
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    all.sort_unstable();
    assert_eq!(all, (1..=4093).collect::<Vec<_>>());
    assert_eq!(ids.allocate(), None);
}
#[test]
fn finite_pool_validates_release_and_tail_bits() {
    let mut pool = Recycle::new(1, 66);
    assert!(!pool.release(1));
    assert_eq!(
        (0..65)
            .map(|_| pool.allocate().unwrap())
            .collect::<Vec<_>>(),
        (1..66).collect::<Vec<_>>()
    );
    assert_eq!(pool.allocate(), None);
    for invalid in [0, 66, usize::MAX] {
        assert!(!pool.release(invalid));
    }
    assert!(pool.release(65));
    assert!(!pool.release(65));
    assert_eq!(pool.allocate(), Some(65));
    assert_eq!(pool.allocate(), None);
}
