use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::machine::{board, hart, layout::MAX_HART_SLOTS};

pub(crate) const DISARMED: u64 = u64::MAX;

// Only the local Hart touches its comparator and cache, with interrupts masked.
static ARMED: [AtomicU64; MAX_HART_SLOTS] = [const { AtomicU64::new(DISARMED) }; MAX_HART_SLOTS];

use sbi::{TimerCall, ecall::SArgs, fid::Timer};

static SUPERVISOR_STIMECMP: AtomicBool = AtomicBool::new(false);
static SSTC: [AtomicBool; MAX_HART_SLOTS] = [const { AtomicBool::new(false) }; MAX_HART_SLOTS];

pub(super) fn configure(supervisor_stimecmp: bool) {
    SUPERVISOR_STIMECMP.store(supervisor_stimecmp, Ordering::Release);
}

pub(crate) fn initialize() {
    let slot = hart::hart_id();
    // The ISA bit alone does not authorize access through menvcfg/mcounteren.
    let direct = SUPERVISOR_STIMECMP.load(Ordering::Acquire) && board::info().hart.sstc(slot);
    SSTC[slot.get()].store(direct, Ordering::Relaxed);
    program(DISARMED);
}

pub(crate) fn set_timer(at: u64) {
    let armed = &ARMED[hart::hart_id().get()];
    if armed.load(Ordering::Relaxed) != at {
        program(at);
        armed.store(at, Ordering::Relaxed);
    } else {
        #[cfg(debug_assertions)]
        SKIPPED.fetch_add(1, Ordering::Relaxed);
    }
}

// An unchanged, elapsed comparator remains pending until reprogrammed.
pub(crate) fn elapsed() -> bool {
    let at = ARMED[hart::hart_id().get()].load(Ordering::Relaxed);
    at != DISARMED && super::now().as_ticks() >= at
}

fn program(at: u64) {
    let direct = SSTC[hart::hart_id().get()].load(Ordering::Relaxed);
    #[cfg(debug_assertions)]
    let start = super::now().as_ticks();
    if direct {
        // Boot firmware authorizes supervisor access on Sstc Harts.
        unsafe { core::arch::asm!("csrw 0x14d, {at}", at = in(reg) at, options(nostack)) };
    } else {
        TimerCall::new(Timer::SetTimer)
            .args(SArgs {
                a0: at as usize,
                ..Default::default()
            })
            .call()
            .unwrap();
    }
    #[cfg(debug_assertions)]
    {
        let spent = super::now().as_ticks().saturating_sub(start);
        let (writes, ticks) = &MEASURES[usize::from(direct)];
        writes.fetch_add(1, Ordering::Relaxed);
        ticks.fetch_add(spent, Ordering::Relaxed);
    }
}

#[cfg(debug_assertions)]
static MEASURES: [(AtomicU64, AtomicU64); 2] =
    [const { (AtomicU64::new(0), AtomicU64::new(0)) }; 2];
#[cfg(debug_assertions)]
static SKIPPED: AtomicU64 = AtomicU64::new(0);

#[cfg(debug_assertions)]
pub(crate) fn measures() -> ([(u64, u64); 2], u64) {
    (
        MEASURES.each_ref().map(|(writes, ticks)| {
            (
                writes.load(Ordering::Relaxed),
                ticks.load(Ordering::Relaxed),
            )
        }),
        SKIPPED.load(Ordering::Relaxed),
    )
}
