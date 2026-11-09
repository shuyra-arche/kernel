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
