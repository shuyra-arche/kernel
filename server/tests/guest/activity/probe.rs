//! Test-owned timing/visit counters. Never compiled into the production provider.
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::*};
static STOPPED: AtomicBool = AtomicBool::new(false);
static TURNS: AtomicU64 = AtomicU64::new(0);
static STEPS: AtomicU64 = AtomicU64::new(0);
static MAX_STEPS: AtomicU64 = AtomicU64::new(0);
static MAX_TURN: AtomicU64 = AtomicU64::new(0);
static MAX_STEP: AtomicU64 = AtomicU64::new(0);
static CAPTURES: AtomicU64 = AtomicU64::new(0);
static ROUNDS: AtomicU64 = AtomicU64::new(0);
static POLLS: AtomicU64 = AtomicU64::new(0);
static HIST: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
static TASKS: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];
pub(crate) fn steps() -> u64 {
    STEPS.load(Relaxed)
}
pub(crate) fn stamp() -> u64 {
    crate::metal::clock::uptime().as_nanos() as u64
}
pub(crate) fn turn(start: u64, steps: u64) {
    assert!(steps <= 8);
    TURNS.fetch_add(1, Relaxed);
    MAX_STEPS.fetch_max(steps, Relaxed);
    MAX_TURN.fetch_max(stamp() - start, Relaxed);
}
pub(crate) fn step(start: u64) {
    let elapsed = stamp() - start;
    STEPS.fetch_add(1, Relaxed);
    MAX_STEP.fetch_max(elapsed, Relaxed);
    let bin = (63 - elapsed.max(1).leading_zeros()) as usize;
    HIST[bin].fetch_add(1, Relaxed);
}
pub(crate) fn capture() {
    CAPTURES.fetch_add(1, Relaxed);
}
pub(crate) fn round() {
    ROUNDS.fetch_add(1, Relaxed);
}
pub(crate) fn poll() {
    POLLS.fetch_add(1, Relaxed);
}
pub(crate) fn task(id: usize) {
    if let Some(mask) = TASKS.get(id) {
        mask.fetch_or(1 << crate::metal::hart::hart_id().get(), Relaxed);
    }
}
pub(crate) fn report() {
    if STOPPED.load(Relaxed) {
        crate::putln!("TRACE activity-observer-stop");
    }
    crate::putln!(
        "TRACE activity-budget turns={} steps={} max_steps={} max_turn_ns={} max_step_ns={} captures={} rounds={} polls={}",
        TURNS.load(Relaxed),
        {
            let n = steps();
            if STOPPED.load(Relaxed) {
                assert!(n > 0);
            }
            n
        },
        MAX_STEPS.load(Relaxed),
        MAX_TURN.load(Relaxed),
        MAX_STEP.load(Relaxed),
        CAPTURES.load(Relaxed),
        ROUNDS.load(Relaxed),
        POLLS.load(Relaxed)
    );
    for (bin, count) in HIST.iter().enumerate() {
        let n = count.load(Relaxed);
        if n > 0 {
            crate::putln!("TRACE activity-step bin={} count={}", bin, n);
        }
    }
    for (id, mask) in TASKS.iter().enumerate() {
        let bits = mask.load(Relaxed);
        if bits > 0 {
            crate::putln!("TRACE activity-task id={} mask={}", id, bits);
        }
    }
}
