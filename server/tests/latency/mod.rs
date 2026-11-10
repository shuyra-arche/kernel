#[path = "../../src/metal/measure/distribution.rs"]
mod distribution;
#[test]
fn distribution_preserves_extreme_ticks_and_concurrent_samples() {
    let d = std::sync::Arc::new(distribution::Distribution::new());
    let samples = [0, 1, 2, 3, 4, 1 << 30, 1 << 31, u64::MAX];
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let d = d.clone();
            std::thread::spawn(move || {
                for _ in 0..1000 {
                    for value in samples {
                        d.record(value);
                    }
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let (count, max, bins) = d.snapshot();
    assert_eq!(count, 32_000);
    assert_eq!(bins.iter().sum::<u64>(), count);
    assert_eq!(max, u64::MAX);
    assert_eq!(&bins[..4], &[4000, 4000, 8000, 4000]);
    assert_eq!(bins[31], 12000);
}

#[path = "../../src/metal/measure/execution.rs"]
mod execution;
#[path = "../../src/metal/measure/transaction.rs"]
mod transaction;

#[test]
fn execution_preserves_queue_origin_and_partitions_one_return() {
    let e = execution::Execution::new();
    e.enqueue(10);
    e.enqueue(20); // transfer does not erase the initial ready publication
    e.dispatch(30);
    e.dispatch(40); // selected again before an actual return
    assert_eq!(e.finish(45), Some((30, 5, 35)));
    assert_eq!(e.finish(46), None);
    e.enqueue(50);
    e.dispatch(60);
    e.cancel();
    assert_eq!(e.finish(70), None);
}

#[test]
fn attribution_retains_bounded_distinct_sites_and_paired_wait() {
    #[track_caller]
    fn sample(hold: u64) -> transaction::Sample {
        let outer = transaction::Transaction {
            site: core::panic::Location::caller(),
            start: 10,
            wait: 3,
        };
        transaction::Sample {
            site: outer.site,
            hold: outer.start + hold,
            wait: outer.wait,
        }
    }
    let samples = [
        sample(1),
        sample(2),
        sample(3),
        sample(4),
        sample(5),
        sample(6),
        sample(7),
        sample(8),
        sample(9),
    ];
    let mut top = transaction::Slowest::new();
    for sample in samples {
        top.record(sample);
    }
    assert_eq!(top.samples.iter().flatten().count(), 8);
    assert!(top.samples.iter().flatten().all(|s| s.hold > 11));
    let mut repeated = samples[8];
    repeated.hold = 100;
    repeated.wait = 27;
    top.record(repeated);
    top.record(samples[8]);
    let found: Vec<_> = top
        .samples
        .iter()
        .flatten()
        .filter(|s| s.site == repeated.site)
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].hold, found[0].wait), (100, 27));
}
