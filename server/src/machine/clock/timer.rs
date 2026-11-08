use core::sync::atomic::{AtomicU64, Ordering};

use crate::machine::{hart, layout::MAX_HART_SLOTS};

pub(crate) const DISARMED: u64 = u64::MAX;

// Only the local Hart touches its comparator and cache, with interrupts masked.
static ARMED: [AtomicU64; MAX_HART_SLOTS] = [const { AtomicU64::new(DISARMED) }; MAX_HART_SLOTS];

use sbi::{TimerCall, ecall::SArgs, fid::Timer};

pub(crate) fn initialize() {
    program(DISARMED);
}

pub(crate) fn set_timer(at: u64) {
    let armed = &ARMED[hart::hart_id().get()];
    if armed.load(Ordering::Relaxed) != at {
        program(at);
        armed.store(at, Ordering::Relaxed);
    }
}

// An unchanged, elapsed comparator remains pending until reprogrammed.
pub(crate) fn elapsed() -> bool {
    let at = ARMED[hart::hart_id().get()].load(Ordering::Relaxed);
    at != DISARMED && super::now().as_ticks() >= at
}

fn program(at: u64) {
    TimerCall::new(Timer::SetTimer)
        .args(SArgs {
            a0: at as usize,
            ..Default::default()
        })
        .call()
        .unwrap();
}
