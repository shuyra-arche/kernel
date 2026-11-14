//! Current native Calls on resident same-Team Tasks. No RAM Publish, alternate
//! ABI, test scheduler or relaxed affinity/budget gate is used.
#![no_std]
#![no_main]
#[allow(dead_code)]
mod support;
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use kernel_api::{
    abi::call::{room::RoomQuery, unit::UnitFail},
    address::VirtAddr,
    model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{AccordRequest, AccordTarget, Distributable, Limits, Operations, PendScope},
        sign::SignRights,
        unit::{
            CreatedUnit, ExitScope, Schedule, ScheduleInfo, UnitCreate, UnitQuery, UnitState,
            UnitTarget,
        },
        wait::Wait,
    },
};
use kernel_client::call::{debug, pend, room, sign, unit};
use support::Console;
static BUDGET_STOP: AtomicBool = AtomicBool::new(false);
static LOW_STARTED: AtomicBool = AtomicBool::new(false);
static LOW_STOP: AtomicBool = AtomicBool::new(false);
static LOW_ACK: AtomicBool = AtomicBool::new(false);
static HIGH_RAN: AtomicBool = AtomicBool::new(false);
static HIGH_TOKEN: AtomicUsize = AtomicUsize::new(0);
static LOW_TOKEN: AtomicUsize = AtomicUsize::new(0);
core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail run"
);
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL realtime {info}");
    exit(1)
}
fn exit(code: u64) -> ! {
    unit::exit(code, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn marker(bytes: &[u8]) {
    assert_eq!(
        debug::put(VirtAddr::new(bytes.as_ptr() as usize), bytes.len()).unwrap(),
        bytes.len()
    );
}
fn wait(mut ready: impl FnMut() -> bool) {
    let end = room::clock() + 5_000_000_000;
    while !ready() {
        assert!(room::clock() < end, "realtime progress stalled");
        match room::sleep_until(room::clock() + 1_000_000) {
            Ok(()) => {}
            Err(error) => {
                assert_eq!(error.source, kernel_api::abi::call::room::RoomFail::Busy);
                room::yield_();
            }
        }
    }
}
fn spawn(entry: extern "C" fn() -> !) -> TaskId {
    let bytes = UnitCreate::Task {
        team: None,
        entry: VirtAddr::new(entry as *const () as usize),
        args: VirtAddr::new(0),
        count: 0,
        stack: VirtAddr::new(0),
    }
    .to_bytes();
    let end = room::clock() + 5_000_000_000;
    loop {
        match unit::create(VirtAddr::new(bytes.as_ptr() as usize)) {
            Ok(CreatedUnit::Task(task)) => return task,
            Err(error) if error.source == UnitFail::Busy => {
                assert!(room::clock() < end);
                room::yield_();
            }
            other => panic!("create: {other:?}"),
        }
    }
}
fn resume(task: TaskId) {
    let end = room::clock() + 5_000_000_000;
    loop {
        match unit::resume(UnitTarget::Task(task)) {
            Ok(()) => return,
            Err(error) if error.source == UnitFail::Busy => {
                assert!(room::clock() < end);
                room::yield_();
            }
            other => panic!("resume: {other:?}"),
        }
    }
}
fn admit(task: TaskId, schedule: Schedule) -> Result<(), UnitFail> {
    unit::admit(task, VirtAddr::new(schedule.to_bytes().as_ptr() as usize)).map_err(|e| e.source)
}
fn profile(task: TaskId) -> ScheduleInfo {
    let mut bytes = [0; ScheduleInfo::WIDTH];
    unit::profile(task, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
    ScheduleInfo::from_bytes(&bytes).unwrap()
}
fn spec(priority: u64, budget: u64, period: u64) -> Schedule {
    Schedule {
        hart: 1,
        priority,
        ceiling: priority,
        budget,
        period,
        latency: 10_000_000,
        extensions: 0,
    }
}
fn join(task: TaskId) {
    assert_eq!(support::join(task).exit.map(|(_, code)| code), Some(0));
}
fn stopped(task: TaskId) {
    let info = support::join(task);
    assert_eq!(
        info.exit.map(|(cause, _)| cause),
        Some(kernel_api::model::unit::ExitCause::Slay)
    );
}
fn gate(task: TaskId) -> (PendId, PendId) {
    let root = sign::create(support::license()).unwrap();
    let request = AccordRequest {
        src: root,
        target: AccordTarget::Task(task),
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: PendScope::Object,
        },
    }
    .to_bytes();
    (
        root,
        pend::accord(VirtAddr::new(request.as_ptr() as usize)).unwrap(),
    )
}
fn listen(token: PendId) {
    let kernel_api::abi::call::sign::SignReply::Triggered { sequence } =
        sign::listen(token, Wait::Infinite).unwrap()
    else {
        panic!("missing retained wake")
    };
    sign::consume(token, sequence).unwrap();
}
extern "C" fn budget() -> ! {
    while !BUDGET_STOP.load(Ordering::Acquire) {
        room::yield_();
    }
    exit(0)
}
extern "C" fn low() -> ! {
    LOW_STARTED.store(true, Ordering::Release);
    // Pure user computation must be preempted; it never voluntarily yields.
    while !LOW_STOP.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    LOW_ACK.store(true, Ordering::Release);
    listen(PendId::mint(LOW_TOKEN.load(Ordering::Acquire)));
    exit(0)
}
extern "C" fn high() -> ! {
    listen(PendId::mint(HIGH_TOKEN.load(Ordering::Acquire)));
    HIGH_RAN.store(true, Ordering::Release);
    LOW_STOP.store(true, Ordering::Release);
    exit(0)
}
extern "C" fn unused() -> ! {
    exit(0)
}
#[unsafe(no_mangle)]
extern "C" fn run(at: usize, count: usize) -> ! {
    support::boot(at, count);
    let harts = room::inspect(RoomQuery::Ordinary).unwrap();
    let worker = spawn(budget);
    let schedule = spec(5, 20_000_000, 1_000_000_000);
    if harts == 1 {
        marker(b"ROOM:steady\n");
        assert_eq!(admit(worker, schedule), Err(UnitFail::Denied));
        unit::stop(UnitTarget::Task(worker)).unwrap();
        stopped(worker);
        marker(b"ROOM:teardown\n");
        let _ = writeln!(Console, "PASS realtime harts=1 unsupported=1");
        exit(0)
    }
    let low_task = spawn(low);
    let high_task = spawn(high);
    let excess = spawn(unused);
    let (low_gate, low_token) = gate(low_task);
    LOW_TOKEN.store(low_token.get(), Ordering::Release);
    let (high_gate, high_token) = gate(high_task);
    HIGH_TOKEN.store(high_token.get(), Ordering::Release);
    assert_eq!(admit(worker, schedule), Ok(()));
    assert_eq!(room::inspect(RoomQuery::Ordinary).unwrap(), harts - 1);
    marker(b"ROOM:steady\n");
    resume(worker);
    assert_eq!(unit::evict(worker).unwrap_err().source, UnitFail::Denied);
    wait(|| profile(worker).remaining == 0);
    let exhausted = profile(worker);
    assert!(exhausted.consumed >= schedule.budget);
    let end = room::clock() + 5_000_000_000;
    loop {
        match unit::freeze(UnitTarget::Task(worker)) {
            Ok(()) => break,
            Err(error) if error.source == UnitFail::Busy => {
                assert!(room::clock() < end);
                room::yield_();
            }
            other => panic!("freeze: {other:?}"),
        }
    }
    wait(|| {
        support::task_info(UnitQuery::Target(UnitTarget::Task(worker))).state == UnitState::Frozen
    });
    assert_eq!(profile(worker).remaining, 0);
    assert_eq!(admit(worker, schedule), Err(UnitFail::Denied));
    resume(worker);
    wait(|| {
        let p = profile(worker);
        p.replenishment > exhausted.replenishment && p.consumed > exhausted.consumed
    });
    BUDGET_STOP.store(true, Ordering::Release);
    join(worker);
    wait(|| room::inspect(RoomQuery::Ordinary).unwrap() == harts);
    let slow = spec(1, 4_000_000_000, 10_000_000_000);
    let fast = spec(9, 1_000_000_000, 10_000_000_000);
    assert_eq!(admit(low_task, slow), Ok(()));
    assert_eq!(admit(high_task, fast), Ok(()));
    assert_eq!(
        admit(excess, spec(10, 9_990_000_000, 10_000_000_000)),
        Err(UnitFail::Busy)
    );
    assert_eq!(profile(low_task).schedule, slow);
    unit::stop(UnitTarget::Task(excess)).unwrap();
    stopped(excess);
    resume(high_task);
    wait(|| {
        support::task_info(UnitQuery::Target(UnitTarget::Task(high_task))).state
            == UnitState::Waiting
    });
    resume(low_task);
    wait(|| LOW_STARTED.load(Ordering::Acquire));
    let before = profile(low_task);
    assert!(before.remaining > 0);
    sign::trigger(high_gate).unwrap();
    wait(|| HIGH_RAN.load(Ordering::Acquire) && LOW_ACK.load(Ordering::Acquire));
    wait(|| {
        support::task_info(UnitQuery::Target(UnitTarget::Task(low_task))).state
            == UnitState::Waiting
    });
    let after = profile(low_task);
    assert_eq!(after.schedule.hart, 1);
    assert_eq!(after.replenishment, before.replenishment);
    assert!(
        after.remaining > 0,
        "high priority waited for low budget exhaustion"
    );
    sign::trigger(low_gate).unwrap();
    join(high_task);
    join(low_task);
    wait(|| room::inspect(RoomQuery::Ordinary).unwrap() == harts);
    marker(b"ROOM:teardown\n");
    sign::delete(low_gate).unwrap();
    sign::delete(high_gate).unwrap();
    let _ = writeln!(
        Console,
        "PASS realtime harts={harts} budget=1 replenishment=1 freeze=1 no-credit-reset=1 preemption=1 fixed-hart=1 overload=denied ordinary-pool=restored"
    );
    exit(0)
}
