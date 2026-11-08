use crate::{Console, lifecycle};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use kernel_api::{address::VirtAddr, model::unit::UnitTarget};
use kernel_client::call::{room, unit};

static STOP: AtomicBool = AtomicBool::new(false);
static READY: AtomicUsize = AtomicUsize::new(0);
static TURN: AtomicUsize = AtomicUsize::new(0);
static STAMP: AtomicU64 = AtomicU64::new(0);
const HANDOFFS: usize = 64;

extern "C" fn load() -> ! {
    READY.fetch_add(1, Ordering::Release);
    while !STOP.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    unit::exit(0, 0, VirtAddr::new(0), 0)
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
extern "C" fn clock_handoff_send() {
    core::arch::naked_asm!("nop", "ret");
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
extern "C" fn clock_handoff_receive() {
    core::arch::naked_asm!("nop", "ret");
}

fn exchange(clock: &room::MonotonicClock, mut turn: usize, checkpoint: extern "C" fn()) -> bool {
    let until = clock.now() + 1_000_000_000;
    for _ in 0..HANDOFFS {
        while TURN.load(Ordering::Acquire) != turn {
            if clock.now() >= until {
                return false;
            }
            core::hint::spin_loop();
        }
        checkpoint();
        let sample = clock.now();
        if sample < STAMP.load(Ordering::Relaxed) {
            return false;
        }
        STAMP.store(sample, Ordering::Relaxed);
        TURN.store(turn + 1, Ordering::Release);
        turn += 2;
    }
    true
}

// The test capsule runs in S-mode. The closure must only read the negotiated
// fast clock and shared atomics. Restore the previous SIE bit before any kernel call.
fn on_current_hart(exchange: impl FnOnce() -> bool) -> bool {
    const SIE: usize = 1 << 1;
    let previous: usize;
    unsafe {
        core::arch::asm!("csrrc {previous}, sstatus, {mask}", previous = out(reg) previous, mask = in(reg) SIE, options(nostack))
    };
    let passed = exchange();
    unsafe {
        core::arch::asm!("csrs sstatus, {mask}", mask = in(reg) previous & SIE, options(nostack))
    };
    passed
}

extern "C" fn receive() -> ! {
    let clock = room::MonotonicClock::new();
    assert!(clock.is_fast());
    let passed = on_current_hart(|| {
        TURN.store(1, Ordering::Release);
        exchange(&clock, 2, clock_handoff_receive)
    });
    assert!(
        passed,
        "receiving Hart did not observe monotonic clock handoffs"
    );
    unit::exit(0, 0, VirtAddr::new(0), 0)
}

fn check_cross_hart(clock: &room::MonotonicClock) {
    let peer = lifecycle::spawn(receive as *const () as usize);
    unit::resume(UnitTarget::Task(peer)).unwrap();
    let ready_by = clock.now() + 1_000_000_000;
    while TURN.load(Ordering::Acquire) != 1 {
        assert!(clock.now() < ready_by, "clock peer did not start");
        room::yield_();
    }
    // The peer published readiness with interrupts masked and remains on its
    // Hart until this exchange completes. The sender must be on another Hart.
    assert!(
        on_current_hart(|| exchange(clock, 1, clock_handoff_send)),
        "sending Hart did not observe monotonic clock handoffs"
    );
    assert!(lifecycle::wait_task(peer));
    assert_eq!(lifecycle::result(peer), (1, 0));
    lifecycle::delete(peer);
}

pub(crate) fn run() {
    let workers = core::array::from_fn::<_, 4, _>(|_| {
        let task = lifecycle::spawn(load as *const () as usize);
        unit::resume(UnitTarget::Task(task)).unwrap();
        task
    });
    let ready_by = room::clock() + 1_000_000_000;
    while READY.load(Ordering::Acquire) != workers.len() {
        let now = room::clock();
        assert!(now < ready_by, "load Tasks failed to start");
        room::sleep_until(now + 1_000_000).unwrap();
    }
    let clock = room::MonotonicClock::new();
    let mut last = 0;
    let mut worst = 0;
    let mut total = 0;
    for turn in 0..64 {
        let deadline = room::clock() + 200_201 + (turn * 7919 % 17) * 100_000;
        room::sleep_until(deadline).unwrap();
        let before = room::clock();
        let now = clock.now();
        let after = room::clock();
        assert!(deadline <= before && before <= now && now <= after);
        assert!(last <= now, "clock went backwards across a wake");
        last = now;
        let late = before - deadline;
        worst = worst.max(late);
        total += late;
    }
    STOP.store(true, Ordering::Release);
    for task in workers {
        assert!(lifecycle::wait_task(task));
        lifecycle::delete(task);
    }
    if clock.is_fast() {
        check_cross_hart(&clock);
    }
    let handoffs = if clock.is_fast() { HANDOFFS } else { 0 };

    const SAMPLES: u64 = 4096;
    let start = room::clock();
    for _ in 0..SAMPLES {
        core::hint::black_box(room::clock());
    }
    let trap_ns = room::clock() - start;
    let start = room::clock();
    for _ in 0..SAMPLES {
        core::hint::black_box(clock.now());
    }
    let cached_ns = room::clock() - start;
    let mut line = Line {
        bytes: [0; 256],
        len: 0,
    };
    writeln!(
        line,
        "PASS clock monotonic=1 fast={} samples={SAMPLES} envcall_ns={trap_ns} cached_ns={cached_ns} wakes=64 late_sum_ns={total} late_max_ns={worst} handoffs={handoffs}",
        usize::from(clock.is_fast())
    )
    .unwrap();
    Console
        .write_str(core::str::from_utf8(&line.bytes[..line.len]).unwrap())
        .unwrap();
}

struct Line {
    bytes: [u8; 256],
    len: usize,
}

impl Write for Line {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len + text.len();
        self.bytes
            .get_mut(self.len..end)
            .ok_or(core::fmt::Error)?
            .copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
