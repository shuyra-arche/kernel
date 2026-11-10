//! QEMU virt UART is a real level interrupt, not a software Sign trigger.
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
use kernel_api::model::unit::ExitScope;
use kernel_api::{
    abi::reply::SignReply,
    address::VirtAddr,
    boot::{
        args,
        ledger::{Entries, Name},
    },
    model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{AccordRequest, AccordTarget, Distributable, Limits, Operations, PendRange},
        sign::SignRights,
        unit::{UnitQuery, UnitState, UnitTarget},
        wait::Wait,
    },
};
use kernel_client::call::{pend, room, share_view, sign, unit};
static LOAD_STOP: AtomicBool = AtomicBool::new(false);
static LOAD_ENTERED: AtomicUsize = AtomicUsize::new(0);
struct Load([TaskId; 4]);
impl Load {
    fn start() -> Self {
        LOAD_STOP.store(false, Ordering::Release);
        LOAD_ENTERED.store(0, Ordering::Release);
        let tasks = core::array::from_fn(|_| {
            let task = super::lifecycle::spawn(load_worker as *const () as usize);
            unit::resume(UnitTarget::Task(task)).unwrap();
            task
        });
        let deadline = room::clock() + 3_000_000_000;
        while LOAD_ENTERED.load(Ordering::Acquire) != tasks.len() {
            assert!(room::clock() < deadline, "load workers did not start");
            room::yield_();
        }
        Self(tasks)
    }
    fn finish(self) {
        LOAD_STOP.store(true, Ordering::Release);
        for task in self.0 {
            assert!(super::lifecycle::wait_task(task));
            assert_eq!(super::lifecycle::result(task), (1, 0));
            super::lifecycle::delete(task);
        }
    }
}
extern "C" fn load_worker() -> ! {
    LOAD_ENTERED.fetch_add(1, Ordering::Release);
    while !LOAD_STOP.load(Ordering::Acquire) {
        room::yield_();
    }
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

static UART: AtomicUsize = AtomicUsize::new(0);
static TOKEN: AtomicUsize = AtomicUsize::new(0);
static VIEW: AtomicUsize = AtomicUsize::new(0);
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static OBSERVER: AtomicUsize = AtomicUsize::new(0);
static OBSERVED: AtomicBool = AtomicBool::new(false);
extern "C" fn observer() -> ! {
    let token = PendId::mint(OBSERVER.load(Ordering::Acquire));
    assert!(matches!(
        sign::listen(token, Wait::Infinite).unwrap(),
        SignReply::Triggered { .. }
    ));
    OBSERVED.store(true, Ordering::Release);
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
static ENTERED: AtomicU32 = AtomicU32::new(0);
static REVOKE_FINISHED: AtomicBool = AtomicBool::new(false);
static WATCH_TOKEN: AtomicUsize = AtomicUsize::new(0);
static WATCH_ENTERED: AtomicU32 = AtomicU32::new(0);
static WATCH_OBSERVED: AtomicU32 = AtomicU32::new(0);
extern "C" fn watch() -> ! {
    let token = PendId::mint(WATCH_TOKEN.load(Ordering::Acquire));
    WATCH_ENTERED.fetch_add(1, Ordering::Release);
    room::wake_value(VirtAddr::new(WATCH_ENTERED.as_ptr() as usize), true).unwrap();
    // The controller first confirms every observer is blocked here, then starts
    // contention and the IRQ. Its shared delivery deadline bounds all observers;
    // no observer spends an IRQ service timeout waiting for fixture preparation.
    assert!(matches!(
        sign::listen(token, Wait::Infinite).unwrap(),
        SignReply::Triggered { .. }
    ));
    assert!(sign::consume(token, 1).is_err());
    WATCH_OBSERVED.fetch_add(1, Ordering::Release);
    room::wake_value(VirtAddr::new(WATCH_OBSERVED.as_ptr() as usize), true).unwrap();
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn write(offset: usize, byte: u8) {
    unsafe {
        core::arch::asm!("fence iorw,iorw", options(nostack));
        ((UART.load(Ordering::Acquire) + offset) as *mut u8).write_volatile(byte);
        core::arch::asm!("fence iorw,iorw", options(nostack));
    }
}
fn read(offset: usize) -> u8 {
    unsafe {
        core::arch::asm!("fence iorw,iorw", options(nostack));
        ((UART.load(Ordering::Acquire) + offset) as *const u8).read_volatile()
    }
}
#[unsafe(no_mangle)]
#[inline(never)]
extern "C" fn irq_received() {
    core::hint::black_box(());
}
#[track_caller]
fn receive(token: PendId, wait: Wait) -> u64 {
    let reply = sign::listen(token, wait).unwrap();
    let SignReply::Triggered { sequence } = reply else {
        // Let load Tasks exit after a failed assertion, so shutdown can retain
        // the router counters instead of hiding the failure behind a watchdog.
        LOAD_STOP.store(true, Ordering::Release);
        panic!(
            "UART IRQ missing: {reply:?}; ier={} iir={}",
            read(1),
            read(2)
        )
    };
    irq_received();
    sequence
}
fn deadline() -> Wait {
    Wait::At(room::clock().saturating_add(2_000_000_000))
}

fn retained(token: PendId) -> u64 {
    // Poll never registers a waiter. Prove retention without assuming an IRQ
    // must cross a contended commit boundary within an arbitrary 2 ms sleep.
    let until = room::clock().saturating_add(2_000_000_000);
    loop {
        match sign::listen(token, Wait::Poll).unwrap() {
            SignReply::Triggered { sequence } => {
                irq_received();
                return sequence;
            }
            SignReply::Pending => {
                assert!(
                    room::clock() < until,
                    "unobserved UART IRQ was not retained"
                );
                room::yield_();
            }
            reply => panic!("unexpected UART reply: {reply:?}"),
        }
    }
}
fn accord(
    root: PendId,
    target: AccordTarget,
    operations: Operations,
    modifiable: Modifiable,
) -> Option<PendId> {
    let bytes = AccordRequest {
        src: root,
        target,
        mark: Mark::NONE,
        limits: Limits {
            operations,
            modifiable,
            distributable: Distributable::No,
            range: PendRange::Inherit,
        },
    }
    .to_bytes();
    pend::accord(super::ptr(&bytes)).ok()
}
fn sign_branch(root: PendId, target: AccordTarget) -> PendId {
    accord(
        root,
        target,
        Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
        Modifiable::No,
    )
    .unwrap()
}
fn pair(view: PendId, sign: PendId, task: TaskId) -> (PendId, PendId) {
    let view = accord(
        view,
        AccordTarget::Task(task),
        Operations::Share(
            kernel_api::model::pend::ShareRights::OPEN | kernel_api::model::pend::ShareRights::SHUT,
        ),
        Modifiable::Yes,
    )
    .unwrap();
    let Some(sign) = accord(
        sign,
        AccordTarget::Task(task),
        Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
        Modifiable::No,
    ) else {
        pend::revoke(task, view).unwrap();
        panic!("dispatch rolled back")
    };
    (view, sign)
}
fn open(view: PendId) {
    let (address, _) = share_view::open(view).unwrap();
    UART.store(address.get(), Ordering::Release);
    write(1, 0);
}
fn initial(sign: PendId) {
    let SignReply::Triggered { sequence } = sign::listen(sign, deadline()).unwrap() else {
        panic!("initial device inspection missing")
    };
    sign::consume(sign, sequence).unwrap();
}
extern "C" fn dying_driver() -> ! {
    let token = PendId::mint(TOKEN.load(Ordering::Acquire));
    open(PendId::mint(VIEW.load(Ordering::Acquire)));
    initial(token);
    write(1, 2);
    let sequence = receive(token, deadline());
    SEQUENCE.store(sequence as usize, Ordering::Release);
    // Exit with device asserted and controller claim held.
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
extern "C" fn revoked_driver() -> ! {
    let token = PendId::mint(TOKEN.load(Ordering::Acquire));
    open(PendId::mint(VIEW.load(Ordering::Acquire)));
    initial(token);
    ENTERED.store(1, Ordering::Release);
    room::wake_value(VirtAddr::new(ENTERED.as_ptr() as usize), true).unwrap();
    // This is a revocation experiment: the controller first observes this
    // actual blocked listener. Only authority loss may release it.
    let error = sign::listen(token, Wait::Infinite).unwrap_err();
    assert!(matches!(
        error.source,
        kernel_api::abi::call::sign::SignFail::Dead | kernel_api::abi::call::sign::SignFail::Denied
    ));
    // Keep this ledger alive until the controller finishes exact-identity
    // retries; Task exit would otherwise race the same physical retirement.
    while !REVOKE_FINISHED.load(Ordering::Acquire) {
        room::yield_();
    }
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
pub(super) fn run(at: usize, count: usize) {
    assert_eq!(count, args::LEN);
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let ledger = Entries::new(unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    })
    .unwrap();
    let ((kind, view), sign) = ledger
        .find(Name::Interrupt {
            controller: 0xc000000,
            source: 10,
        })
        .expect("UART dispatch")
        .dispatch()
        .expect("(View, Sign)");
    assert_eq!(kind, kernel_api::model::view::ViewKind::Share);
    let page = PendId::mint(view);
    let root = PendId::mint(sign);
    open(page);
    assert_ne!(read(5) & 0x20, 0);
    assert!(sign::listen(root, Wait::Poll).is_err());
    let load = Load::start();
    let token = sign_branch(root, AccordTarget::Caller);
    assert!(
        accord(
            root,
            AccordTarget::Caller,
            Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
            Modifiable::No
        )
        .is_none()
    );
    assert!(sign::trigger(token).is_err());
    // Assert before the initial receipt: unmask must expose an already-pending
    // source without any subsequent device write creating another edge.
    write(1, 2);
    initial(token);
    let pending = receive(token, deadline());
    write(1, 0);
    sign::consume(token, pending).unwrap();
    load.finish();
    // Prepare observers without the synthetic Yield load. Both the initial
    // pending IRQ above and actual notification delivery below retain that load.
    // More listeners than a notification batch must all progress. Observation
    // and forwarding preserve pending work; only the processing branch consumes.
    let watchers: [TaskId; 6] = core::array::from_fn(|index| {
        let task = super::lifecycle::spawn(watch as *const () as usize);
        let observer = accord(
            root,
            AccordTarget::Task(task),
            Operations::Sign(SignRights::LISTEN),
            Modifiable::No,
        )
        .unwrap();
        WATCH_TOKEN.store(observer.get(), Ordering::Release);
        unit::resume(UnitTarget::Task(task)).unwrap();
        let until = room::clock() + 2_000_000_000;
        loop {
            let entered = WATCH_ENTERED.load(Ordering::Acquire);
            if entered == (index + 1) as u32 {
                break;
            }
            assert!(room::clock() < until, "IRQ observer {index} did not enter");
            crate::room::wait(&WATCH_ENTERED, entered, Wait::At(until));
        }
        // Entry acknowledges ownership of WATCH_TOKEN. Waiting proves the
        // observer has registered on Sign, not merely reached a staging gate.
        while super::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task))).state
            != UnitState::Waiting
        {
            assert!(room::clock() < until, "IRQ observer {index} did not block");
            room::yield_();
        }
        task
    });
    let routed = sign::create(crate::license::get()).unwrap();
    let source = kernel_api::model::sign::SignSource::Sign { token };
    sign::attach(routed, source, 0).unwrap();
    assert_eq!(WATCH_OBSERVED.load(Ordering::Acquire), 0);
    let load = Load::start();
    // This batch checks delivery to all observers under contention. Latency
    // is measured separately, so allow host scheduling headroom for the batch.
    let until = room::clock().saturating_add(5_000_000_000);
    write(1, 2);
    assert_eq!(
        sign::listen(routed, Wait::At(until)).unwrap(),
        SignReply::Source { source, fail: None }
    );
    let observed = receive(token, Wait::At(until));
    loop {
        let seen = WATCH_OBSERVED.load(Ordering::Acquire);
        if seen == watchers.len() as u32 {
            break;
        }
        assert!(
            room::clock() < until,
            "UART notification batch stalled: observed {seen}/6"
        );
        crate::room::wait(&WATCH_OBSERVED, seen, Wait::At(until));
    }
    for task in watchers {
        assert!(super::lifecycle::wait_task(task));
        assert_eq!(super::lifecycle::result(task), (1, 0));
        super::lifecycle::delete(task);
    }
    write(1, 0);
    sign::consume(token, observed).unwrap();
    sign::delete(routed).unwrap();
    super::lifecycle::return_pend(routed);
    let unrelated = sign::create(crate::license::get()).unwrap();
    let observer_task = super::lifecycle::spawn(observer as *const () as usize);
    let observer_token = accord(
        unrelated,
        AccordTarget::Task(observer_task),
        Operations::Sign(SignRights::LISTEN),
        Modifiable::No,
    )
    .unwrap();
    OBSERVER.store(observer_token.get(), Ordering::Release);
    unit::resume(UnitTarget::Task(observer_task)).unwrap();
    let mut previous = 0;
    for turn in 0..24 {
        write(1, 2);
        let sequence = if turn == 0 {
            retained(token)
        } else {
            receive(token, deadline())
        };
        assert!(sequence > previous);
        assert_eq!(
            sign::listen(token, Wait::Poll).unwrap(),
            SignReply::Triggered { sequence }
        );
        assert_eq!(
            sign::listen(unrelated, Wait::Poll).unwrap(),
            SignReply::Pending
        );
        assert_eq!(read(2) & 0x0f, 2, "hardware THRE cause");
        write(1, 0);
        assert!(sign::consume(token, previous).is_err());
        sign::consume(token, sequence).unwrap();
        assert!(sign::consume(token, sequence).is_err());
        previous = sequence;
    }
    write(1, 2);
    let held = receive(token, deadline());
    write(1, 0);
    write(1, 2);
    sign::consume(token, held).unwrap();
    let repeated = receive(token, deadline());
    assert!(repeated > held);
    write(1, 0);
    sign::consume(token, repeated).unwrap();
    super::lifecycle::return_pend(token);
    assert!(!OBSERVED.load(Ordering::Acquire));
    sign::trigger(unrelated).unwrap();
    assert!(super::lifecycle::wait_task(observer_task));
    assert_eq!(super::lifecycle::result(observer_task), (1, 0));
    super::lifecycle::delete(observer_task);
    assert!(OBSERVED.load(Ordering::Acquire));
    sign::delete(unrelated).unwrap();
    super::lifecycle::return_pend(unrelated);
    let task = super::lifecycle::spawn(dying_driver as *const () as usize);
    // Sign cannot be handed out before the matching View exists in this Task.
    assert!(
        accord(
            root,
            AccordTarget::Task(task),
            Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
            Modifiable::No
        )
        .is_none()
    );
    let (view, branch) = pair(page, root, task);
    VIEW.store(view.get(), Ordering::Release);
    TOKEN.store(branch.get(), Ordering::Release);
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(super::lifecycle::wait_task(task));
    assert_eq!(super::lifecycle::result(task), (1, 0));
    super::lifecycle::delete(task);
    open(page);
    let replacement = sign_branch(root, AccordTarget::Caller);
    assert!(sign::consume(replacement, SEQUENCE.load(Ordering::Acquire) as u64).is_err());
    initial(replacement);
    write(1, 2);
    let sequence = receive(replacement, deadline());
    write(1, 0);
    sign::consume(replacement, sequence).unwrap();
    super::lifecycle::return_pend(replacement);
    // Independently revoking either half closes the binding and wakes its waiter.
    for revoke_view in [false, true] {
        ENTERED.store(0, Ordering::Release);
        REVOKE_FINISHED.store(false, Ordering::Release);
        let task = super::lifecycle::spawn(revoked_driver as *const () as usize);
        let (view, branch) = pair(page, root, task);
        VIEW.store(view.get(), Ordering::Release);
        TOKEN.store(branch.get(), Ordering::Release);
        unit::resume(UnitTarget::Task(task)).unwrap();
        // Mapping/initial inspection are setup, not the revocation deadline.
        // Park the controller instead of adding another competing Yield loop.
        while ENTERED.load(Ordering::Acquire) == 0 {
            crate::room::wait(&ENTERED, 0, Wait::Infinite);
        }
        let until = room::clock() + 2_000_000_000;
        while super::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task))).state
            != UnitState::Waiting
        {
            assert!(room::clock() < until, "revocation listener did not block");
            room::yield_();
        }
        let token = if revoke_view { view } else { branch };
        let until = room::clock() + 5_000_000_000;
        loop {
            match pend::revoke(task, token) {
                Ok(()) => break,
                Err(error) => {
                    assert_eq!(error.source, kernel_api::abi::call::pend::PendFail::Busy);
                    assert!(room::clock() < until, "IRQ grant retirement stalled");
                    room::yield_();
                }
            }
        }
        REVOKE_FINISHED.store(true, Ordering::Release);
        assert!(super::lifecycle::wait_task(task));
        assert_eq!(super::lifecycle::result(task), (1, 0));
        super::lifecycle::delete(task);
        open(page);
        let next = sign_branch(root, AccordTarget::Caller);
        initial(next);
        write(1, 2);
        let sequence = receive(next, deadline());
        write(1, 0);
        sign::consume(next, sequence).unwrap();
        super::lifecycle::return_pend(next);
    }
    load.finish();
    share_view::shut(page).unwrap();
    writeln!(super::Console, "PASS interrupt uart=1 retained=1 repeated=24 isolated=1 stale=1 death=1 revoke=1 rebind=1 pair=1 observers=6 forward=1 masked=1").unwrap();
}
