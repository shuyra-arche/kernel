#[path = "../../src/basis/room/shutdown/progress.rs"]
mod progress;
use progress::Progress;
#[test]
fn concurrent_observers_can_claim_only_one_stall_report() {
    let state = std::sync::Mutex::new(Progress::Idle);
    assert!(!state.lock().unwrap().observe(0, (2, 1), 100));
    std::thread::scope(|scope| {
        let observers: Vec<_> = (0..16)
            .map(|_| scope.spawn(|| state.lock().unwrap().observe(100, (2, 1), 100)))
            .collect();
        assert_eq!(
            observers
                .into_iter()
                .map(|t| usize::from(t.join().unwrap()))
                .sum::<usize>(),
            1
        );
    });
    assert!(!state.lock().unwrap().observe(200, (3, 2), 100));
}
#[test]
fn progress_restarts_the_interval_without_truncating_counts() {
    let mut state = Progress::Idle;
    assert!(!state.observe(0, (2, 1), 100));
    assert!(!state.observe(99, (2, 1), 100));
    assert!(!state.observe(100, (2 + (1usize << 32), 1), 100));
    assert!(!state.observe(199, (2 + (1usize << 32), 1), 100));
    assert!(state.observe(200, (2 + (1usize << 32), 1), 100));
    let mut state = Progress::Idle;
    assert!(!state.observe(10, (2, 1), 100));
    assert!(!state.observe(200, (2, 2), 100));
    assert_eq!(state, Progress::Idle);
}
