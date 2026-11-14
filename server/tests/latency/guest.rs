//! ABI 11 exploratory Room workload, composed by run.py with the pinned native
//! fixture. The support module is from that fixture, not a model of the kernel.
#![no_std]
#![no_main]
#[allow(dead_code)]
mod support;
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use kernel_api::{
    address::VirtAddr,
    model::unit::{ExitScope, UnitTarget},
};
use kernel_client::call::{room, unit};
use support::{Console, spawn};
const WORKERS: usize = 8;
const TURNS: usize = 512;
static ARRIVED: AtomicUsize = AtomicUsize::new(0);
static START: AtomicBool = AtomicBool::new(false);
static DONE: AtomicUsize = AtomicUsize::new(0);
static STEPS: AtomicUsize = AtomicUsize::new(0);
core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail run"
);
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL room {info}");
    unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
extern "C" fn worker() -> ! {
    ARRIVED.fetch_add(1, Ordering::Release);
    while !START.load(Ordering::Acquire) {
        room::yield_();
    }
    for turn in 0..TURNS {
        STEPS.fetch_add(1, Ordering::Relaxed);
        room::yield_();
        if turn % 64 == 0 {
            room::sleep_until(room::clock() + 100_000).unwrap();
        }
    }
    DONE.fetch_add(1, Ordering::Release);
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
#[unsafe(no_mangle)]
extern "C" fn run(at: usize, count: usize) -> ! {
    support::boot(at, count);
    let tasks = core::array::from_fn::<_, WORKERS, _>(|_| {
        let task = spawn(worker);
        unit::resume(UnitTarget::Task(task)).unwrap();
        task
    });
    let deadline = room::clock() + 60_000_000_000;
    while ARRIVED.load(Ordering::Acquire) != WORKERS {
        assert!(room::clock() < deadline, "workers did not reach start");
        room::yield_();
    }
    let _ = writeln!(Console, "START room workers={WORKERS} turns={TURNS}");
    let start = room::clock();
    START.store(true, Ordering::Release);
    while DONE.load(Ordering::Acquire) != WORKERS {
        assert!(room::clock() < deadline, "worker progress stalled");
        room::sleep_until(room::clock() + 1_000_000).unwrap();
    }
    let elapsed = room::clock() - start;
    assert_eq!(STEPS.load(Ordering::Relaxed), WORKERS * TURNS);
    for task in tasks {
        assert_eq!(support::join(task).exit.map(|(_, code)| code), Some(0));
    }
    let _ = writeln!(
        Console,
        "PASS room workers={WORKERS} turns={TURNS} steps={} elapsed_ns={elapsed}",
        WORKERS * TURNS
    );
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
