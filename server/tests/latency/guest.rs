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
    abi::call::unit::UnitFail,
    address::VirtAddr,
    model::{
        identity::TaskId,
        unit::{CreatedUnit, ExitScope, UnitCreate, UnitTarget},
    },
};
use kernel_client::call::{room, unit};
use support::Console;
const WORKERS: usize = 8;
const TURNS: usize = 512;
static ARRIVED: AtomicUsize = AtomicUsize::new(0);
static START: AtomicBool = AtomicBool::new(false);
static DONE: AtomicUsize = AtomicUsize::new(0);
static STEPS: AtomicUsize = AtomicUsize::new(0);
static CREATE_RETRIES: AtomicUsize = AtomicUsize::new(0);
core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail run"
);
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL room {info}");
    // Release already-created workers even if setup failed. Their exit cannot
    // turn the retained FAIL marker into a successful workload.
    START.store(true, Ordering::Release);
    unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn spawn(deadline: u64) -> TaskId {
    let bytes = UnitCreate::Task {
        team: None,
        entry: VirtAddr::new(worker as *const () as usize),
        args: VirtAddr::new(0),
        count: 0,
        stack: VirtAddr::new(0),
    }
    .to_bytes();
    loop {
        match unit::create(VirtAddr::new(bytes.as_ptr() as usize)) {
            Ok(CreatedUnit::Task(task)) => return task,
            Err(error) if error.source == UnitFail::Busy => {
                CREATE_RETRIES.fetch_add(1, Ordering::Relaxed);
                assert!(room::clock() < deadline, "Task creation stayed Busy");
                room::yield_();
            }
            result => panic!("Task creation: {result:?}"),
        }
    }
}
fn sleep(at: u64) {
    loop {
        match room::sleep_until(at) {
            Ok(()) => return,
            Err(error) if error.source == kernel_api::abi::call::room::RoomFail::Busy => {
                if room::clock() >= at {
                    return;
                }
                room::yield_();
            }
            error => panic!("sleep: {error:?}"),
        }
    }
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
            sleep(room::clock() + 100_000);
        }
    }
    DONE.fetch_add(1, Ordering::Release);
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
#[unsafe(no_mangle)]
extern "C" fn run(at: usize, count: usize) -> ! {
    support::boot(at, count);
    let deadline = room::clock() + 60_000_000_000;
    let tasks = core::array::from_fn::<_, WORKERS, _>(|_| {
        let task = spawn(deadline);
        unit::resume(UnitTarget::Task(task)).unwrap();
        task
    });
    while ARRIVED.load(Ordering::Acquire) != WORKERS {
        assert!(room::clock() < deadline, "workers did not reach start");
        room::yield_();
    }
    let _ = writeln!(Console, "START room workers={WORKERS} turns={TURNS}");
    let _ = writeln!(
        Console,
        "room-setup: create_busy={}",
        CREATE_RETRIES.load(Ordering::Relaxed)
    );
    let start = room::clock();
    START.store(true, Ordering::Release);
    while DONE.load(Ordering::Acquire) != WORKERS {
        assert!(room::clock() < deadline, "worker progress stalled");
        sleep(room::clock() + 1_000_000);
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
