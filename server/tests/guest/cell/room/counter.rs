use crate::{Console, lifecycle};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use kernel_api::{address::VirtAddr, model::unit::UnitTarget};
use kernel_client::call::{room, unit};

static STOP: AtomicBool = AtomicBool::new(false);
static READY: AtomicUsize = AtomicUsize::new(0);

extern "C" fn load() -> ! {
    READY.fetch_add(1, Ordering::Release);
    while !STOP.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    unit::exit(0, 0, VirtAddr::new(0), 0)
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
extern "C" fn clock_migration_checkpoint() {
    core::arch::naked_asm!("nop", "ret");
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
        clock_migration_checkpoint();
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
        "PASS clock monotonic=1 fast={} samples={SAMPLES} envcall_ns={trap_ns} cached_ns={cached_ns} wakes=64 late_sum_ns={total} late_max_ns={worst}",
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
