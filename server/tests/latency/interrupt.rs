//! Native ABI 11 UART/Sign stress. MMIO is a trusted static initial-Team mapping;
//! Pend authority still owns the real IRQ receiver binding and receipts.
#![no_std]
#![no_main]
#[allow(dead_code)]
mod support;
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use kernel_api::{
    abi::call::{
        pend::PendFail,
        sign::{SignFail, SignReply},
        unit::UnitFail,
    },
    address::VirtAddr,
    boot::{
        args,
        ledger::{Entries, Name},
    },
    model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{AccordRequest, AccordTarget, Distributable, Limits, Operations, ShareRights},
        sign::{SignRights, SignSource},
        unit::{CreatedUnit, ExitScope, UnitCreate, UnitQuery, UnitState, UnitTarget},
        wait::Wait,
    },
};
use kernel_client::call::{debug, pend, room, sign, unit};
use support::Console;
const ROUNDS: usize = 256;
const LOAD: usize = 4;
static STOP: AtomicBool = AtomicBool::new(false);
static RELEASE: AtomicBool = AtomicBool::new(false);
static ARRIVED: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicUsize = AtomicUsize::new(0);
static WATCH_TOKEN: AtomicUsize = AtomicUsize::new(0);
static WATCH_ENTERED: AtomicUsize = AtomicUsize::new(0);
static WATCH_SEEN: AtomicUsize = AtomicUsize::new(0);
static DRIVER_TOKEN: AtomicUsize = AtomicUsize::new(0);
static DRIVER_MODE: AtomicUsize = AtomicUsize::new(0);
static DRIVER_READY: AtomicUsize = AtomicUsize::new(0);
static DRIVER_RELEASE: AtomicBool = AtomicBool::new(false);
static OLD: AtomicUsize = AtomicUsize::new(0);
core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail run"
);
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    STOP.store(true, Ordering::Release);
    RELEASE.store(true, Ordering::Release);
    DRIVER_RELEASE.store(true, Ordering::Release);
    let _ = writeln!(Console, "FAIL irq {info}");
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
fn until() -> u64 {
    room::clock() + 5_000_000_000
}
fn wait(mut ready: impl FnMut() -> bool) {
    let end = until();
    while !ready() {
        assert!(room::clock() < end, "IRQ fixture progress stalled");
        room::yield_();
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
    let end = until();
    loop {
        match unit::create(VirtAddr::new(bytes.as_ptr() as usize)) {
            Ok(CreatedUnit::Task(task)) => return task,
            Err(error) if error.source == UnitFail::Busy => {
                assert!(room::clock() < end);
                room::yield_();
            }
            other => panic!("IRQ worker creation: {other:?}"),
        }
    }
}
fn join(task: TaskId) {
    assert_eq!(support::join(task).exit.map(|(_, code)| code), Some(0));
}
fn grant(
    root: PendId,
    target: AccordTarget,
    operations: Operations,
    mutable: Modifiable,
) -> kernel_api::abi::call::pend::PendResult<PendId> {
    let bytes = AccordRequest {
        src: root,
        target,
        mark: Mark::NONE,
        limits: Limits {
            operations,
            modifiable: mutable,
            distributable: Distributable::No,
            scope: support::pend_info(root).limits.scope,
        },
    }
    .to_bytes();
    pend::accord(VirtAddr::new(bytes.as_ptr() as usize))
}
#[track_caller]
fn required(root: PendId, target: AccordTarget, rights: Operations, mutable: Modifiable) -> PendId {
    let end = until();
    loop {
        match grant(root, target, rights, mutable) {
            Ok(token) => return token,
            Err(error) if error.source == PendFail::Busy => {
                assert!(room::clock() < end);
                room::yield_();
            }
            other => panic!("IRQ grant: {other:?}"),
        }
    }
}
fn release(token: PendId) {
    let end = until();
    loop {
        match pend::return_(token) {
            Ok(()) => return,
            Err(error) if error.source == PendFail::Busy => {
                assert!(room::clock() < end);
                room::yield_();
            }
            other => panic!("IRQ return: {other:?}"),
        }
    }
}
#[track_caller]
fn consumer(root: PendId, target: AccordTarget) -> PendId {
    required(
        root,
        target,
        Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
        Modifiable::No,
    )
}
fn pair(view: PendId, root: PendId, task: TaskId) -> (PendId, PendId) {
    let view = required(
        view,
        AccordTarget::Task(task),
        Operations::Share(ShareRights::ASSOCIATE),
        Modifiable::Yes,
    );
    (view, consumer(root, AccordTarget::Task(task)))
}
fn write(offset: usize, byte: u8) {
    unsafe {
        core::arch::asm!("fence iorw,iorw", options(nostack));
        ((0x10000000 + offset) as *mut u8).write_volatile(byte);
        core::arch::asm!("fence iorw,iorw", options(nostack));
    }
}
fn read(offset: usize) -> u8 {
    unsafe {
        core::arch::asm!("fence iorw,iorw", options(nostack));
        ((0x10000000 + offset) as *const u8).read_volatile()
    }
}
fn receive(token: PendId) -> u64 {
    let SignReply::Triggered { sequence } =
        sign::listen(token, Wait::At(room::clock() + 2_000_000_000)).unwrap()
    else {
        panic!("UART IRQ missing")
    };
    sequence
}
fn initial(token: PendId) {
    let sequence = receive(token);
    sign::consume(token, sequence).unwrap();
}
extern "C" fn load() -> ! {
    for _ in 0..64 {
        room::yield_();
    }
    ARRIVED.fetch_add(1, Ordering::Release);
    while !STOP.load(Ordering::Acquire) {
        room::yield_();
    }
    DONE.fetch_add(1, Ordering::Release);
    while !RELEASE.load(Ordering::Acquire) {
        room::yield_();
    }
    exit(0)
}
extern "C" fn watch() -> ! {
    let token = PendId::mint(WATCH_TOKEN.load(Ordering::Acquire));
    WATCH_ENTERED.fetch_add(1, Ordering::Release);
    assert!(matches!(
        sign::listen(token, Wait::Infinite).unwrap(),
        SignReply::Triggered { .. }
    ));
    assert!(sign::consume(token, 1).is_err());
    WATCH_SEEN.fetch_add(1, Ordering::Release);
    while !RELEASE.load(Ordering::Acquire) {
        room::yield_();
    }
    exit(0)
}
extern "C" fn driver() -> ! {
    let token = PendId::mint(DRIVER_TOKEN.load(Ordering::Acquire));
    initial(token);
    DRIVER_READY.store(1, Ordering::Release);
    if DRIVER_MODE.load(Ordering::Acquire) == 0 {
        write(1, 2);
        OLD.store(receive(token) as usize, Ordering::Release);
        exit(0); // controller claim and pending event remain retained
    }
    let error = sign::listen(token, Wait::Infinite).unwrap_err();
    assert!(matches!(error.source, SignFail::Dead | SignFail::Denied));
    DRIVER_READY.store(2, Ordering::Release);
    while !DRIVER_RELEASE.load(Ordering::Acquire) {
        room::yield_();
    }
    exit(0)
}
#[unsafe(no_mangle)]
extern "C" fn run(at: usize, count: usize) -> ! {
    support::boot(at, count);
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let entries = Entries::new(unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    })
    .unwrap();
    let ((_, view), root) = entries
        .find(Name::Interrupt {
            controller: 0xc000000,
            source: 10,
        })
        .unwrap()
        .dispatch()
        .unwrap();
    let view = PendId::mint(view);
    let root = PendId::mint(root);
    write(1, 0);
    assert_ne!(read(5) & 0x20, 0);
    assert!(sign::listen(root, Wait::Poll).is_err());
    let loads = core::array::from_fn::<_, LOAD, _>(|_| spawn(load));
    let watchers = core::array::from_fn::<_, 6, _>(|_| spawn(watch));
    let drivers = core::array::from_fn::<_, 3, _>(|_| spawn(driver));
    for task in loads {
        unit::resume(UnitTarget::Task(task)).unwrap();
    }
    wait(|| ARRIVED.load(Ordering::Acquire) == LOAD);
    let token = consumer(root, AccordTarget::Caller);
    assert!(
        grant(
            root,
            AccordTarget::Caller,
            Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
            Modifiable::No
        )
        .is_err()
    );
    assert!(sign::trigger(token).is_err());
    write(1, 2);
    initial(token);
    let first = receive(token);
    write(1, 0);
    sign::consume(token, first).unwrap();
    for _ in 0..16 {
        write(1, 2);
        let sequence = receive(token);
        write(1, 0);
        sign::consume(token, sequence).unwrap();
    }
    for (index, task) in watchers.into_iter().enumerate() {
        let observer = required(
            root,
            AccordTarget::Task(task),
            Operations::Sign(SignRights::LISTEN),
            Modifiable::No,
        );
        WATCH_TOKEN.store(observer.get(), Ordering::Release);
        unit::resume(UnitTarget::Task(task)).unwrap();
        wait(|| WATCH_ENTERED.load(Ordering::Acquire) == index + 1);
        wait(|| {
            support::task_info(UnitQuery::Target(UnitTarget::Task(task))).state
                == UnitState::Waiting
        });
    }
    let routed = sign::create(support::license()).unwrap();
    let unrelated = sign::create(support::license()).unwrap();
    let source = SignSource::Sign { token };
    sign::attach(routed, source, 0).unwrap();
    marker(b"ROOM:steady\n");
    let began = room::clock();
    let mut max_ns = 0;
    let mut sum_ns = 0;
    let mut bins = [0u64; 32];
    let mut previous = first;
    for turn in 0..ROUNDS {
        let start = room::clock();
        write(1, 2);
        let sequence = if turn == 0 {
            assert_eq!(
                sign::listen(routed, Wait::At(until())).unwrap(),
                SignReply::Source { source, fail: None }
            );
            let sequence = receive(token);
            wait(|| WATCH_SEEN.load(Ordering::Acquire) == 6);
            sequence
        } else {
            receive(token)
        };
        let ns = room::clock() - start;
        max_ns = max_ns.max(ns);
        sum_ns += ns;
        bins[(64 - ns.leading_zeros() as usize).min(31)] += 1;
        assert!(sequence > previous);
        assert_eq!(
            sign::listen(token, Wait::Poll).unwrap(),
            SignReply::Triggered { sequence }
        );
        assert_eq!(
            sign::listen(unrelated, Wait::Poll).unwrap(),
            SignReply::Pending
        );
        assert_eq!(read(2) & 0xf, 2);
        write(1, 0);
        assert!(sign::consume(token, previous).is_err());
        sign::consume(token, sequence).unwrap();
        assert!(sign::consume(token, sequence).is_err());
        previous = sequence;
    }
    write(1, 2);
    let held = receive(token);
    for _ in 0..16 {
        write(1, 0);
        write(1, 2);
        assert_eq!(
            sign::listen(token, Wait::Poll).unwrap(),
            SignReply::Triggered { sequence: held }
        );
    }
    sign::consume(token, held).unwrap();
    let repeated = receive(token);
    assert!(repeated > held);
    write(1, 0);
    sign::consume(token, repeated).unwrap();
    release(token);
    assert!(
        grant(
            root,
            AccordTarget::Task(drivers[0]),
            Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
            Modifiable::No
        )
        .is_err(),
        "Sign without View must fail"
    );
    for (index, task) in drivers.into_iter().enumerate() {
        DRIVER_READY.store(0, Ordering::Release);
        DRIVER_RELEASE.store(false, Ordering::Release);
        DRIVER_MODE.store(index, Ordering::Release);
        let (child_view, branch) = pair(view, root, task);
        DRIVER_TOKEN.store(branch.get(), Ordering::Release);
        unit::resume(UnitTarget::Task(task)).unwrap();
        if index == 0 {
            join(task);
        } else {
            wait(|| DRIVER_READY.load(Ordering::Acquire) == 1);
            wait(|| {
                support::task_info(UnitQuery::Target(UnitTarget::Task(task))).state
                    == UnitState::Waiting
            });
            let token = if index == 1 { branch } else { child_view };
            let end = until();
            loop {
                match pend::revoke(task, token) {
                    Ok(()) => break,
                    Err(error) if error.source == PendFail::Busy => {
                        assert!(room::clock() < end);
                        room::yield_();
                    }
                    other => panic!("IRQ revoke: {other:?}"),
                }
            }
            wait(|| DRIVER_READY.load(Ordering::Acquire) == 2);
            DRIVER_RELEASE.store(true, Ordering::Release);
            join(task);
        }
        write(1, 0);
        let replacement = consumer(root, AccordTarget::Caller);
        assert!(sign::consume(replacement, OLD.load(Ordering::Acquire) as u64).is_err());
        initial(replacement);
        write(1, 2);
        let sequence = receive(replacement);
        write(1, 0);
        sign::consume(replacement, sequence).unwrap();
        release(replacement);
    }
    let elapsed = room::clock() - began;
    marker(b"ROOM:teardown\n");
    STOP.store(true, Ordering::Release);
    wait(|| DONE.load(Ordering::Acquire) == LOAD);
    RELEASE.store(true, Ordering::Release);
    for task in loads {
        join(task);
    }
    for task in watchers {
        join(task);
    }
    sign::delete(routed).unwrap();
    release(routed);
    sign::delete(unrelated).unwrap();
    release(unrelated);
    let _ = writeln!(
        Console,
        "irq-consumer: count={ROUNDS} max_ns={max_ns} sum_ns={sum_ns} bins={bins:?}"
    );
    let _ = writeln!(
        Console,
        "PASS irq rounds={ROUNDS} load=4 observers=6 retained=1 repeated=1 bursts=16 stale=1 death=1 revoke-sign=1 revoke-view=1 rebind=3 pair=1 forward=1 elapsed_ns={elapsed}"
    );
    exit(0)
}
