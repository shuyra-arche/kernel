#[path = "../../../../src/basis/room/shutdown/progress.rs"]
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

#[path = "../../../../src/basis/room/shutdown/preparation.rs"]
mod preparation;
use preparation::{Claim, Preparation, Unsettled};
#[test]
fn final_barrier_requires_success_and_busy_or_failure_preserves_obligations() {
    let gate = Preparation::new();
    assert_eq!(gate.claim(), Claim::Prepare);
    assert_eq!(gate.claim(), Claim::Wait);
    assert!(!gate.complete(Err(Unsettled::Busy)));
    assert_eq!(gate.claim(), Claim::Prepare);
    assert!(gate.complete(Ok(())));
    for _ in 0..8 {
        assert_eq!(gate.claim(), Claim::Ready);
    }
    let failed = Preparation::new();
    assert_eq!(failed.claim(), Claim::Prepare);
    assert!(!failed.complete(Err(Unsettled::Failed)));
    for _ in 0..8 {
        assert_eq!(failed.claim(), Claim::Failed);
    }
}
#[test]
fn only_one_hart_can_prepare_and_publish_finalization_readiness() {
    let gate = std::sync::Arc::new(Preparation::new());
    let (notice, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let owner = gate.clone();
    let thread = std::thread::spawn(move || {
        assert_eq!(owner.claim(), Claim::Prepare);
        notice.send(()).unwrap();
        wait.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(owner.complete(Ok(())));
    });
    ready
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    std::thread::scope(|scope| {
        for _ in 0..16 {
            let gate = &gate;
            scope.spawn(move || assert_eq!(gate.claim(), Claim::Wait));
        }
    });
    release.send(()).unwrap();
    thread.join().unwrap();
    assert_eq!(gate.claim(), Claim::Ready);
}

#[test]
fn preparation_decisions_reject_completion_outside_the_owned_running_state() {
    use preparation::{Event, InvalidCompletion, State, Transition, decide};
    assert_eq!(
        decide(State::Idle, Event::Request),
        Ok(Transition {
            next: State::Preparing,
            decision: Claim::Prepare,
        })
    );
    for state in [State::Idle, State::Ready, State::Failed] {
        for result in [Ok(()), Err(Unsettled::Busy), Err(Unsettled::Failed)] {
            assert_eq!(
                decide(state, Event::Completed(result)),
                Err(InvalidCompletion)
            );
        }
    }
    assert_eq!(
        decide(State::Preparing, Event::Completed(Err(Unsettled::Busy))),
        Ok(Transition {
            next: State::Idle,
            decision: Claim::Wait,
        })
    );
    assert_eq!(
        decide(State::Preparing, Event::Completed(Err(Unsettled::Failed))),
        Ok(Transition {
            next: State::Failed,
            decision: Claim::Failed,
        })
    );
    assert_eq!(
        decide(State::Preparing, Event::Completed(Ok(()))),
        Ok(Transition {
            next: State::Ready,
            decision: Claim::Ready,
        })
    );
}

mod root;
