//! QEMU virt UART is a real level interrupt, not a software Sign trigger.
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::{
    abi::reply::SignReply,
    address::VirtAddr,
    boot::{
        args,
        ledger::{Entries, Name, Page},
    },
    model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{AccordRequest, AccordTarget, Distributable, Limits, Operations, PendRange},
        sign::SignRights,
        unit::UnitTarget,
        wait::Wait,
    },
};
use kernel_client::call::{pend, room, share_view, sign, unit};
static UART: AtomicUsize = AtomicUsize::new(0);
static TOKEN: AtomicUsize = AtomicUsize::new(0);
static GENERATION: AtomicUsize = AtomicUsize::new(0);
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static ENTERED: AtomicUsize = AtomicUsize::new(0);
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
fn receive(token: PendId, wait: Wait) -> u64 {
    let SignReply::Triggered { sequence } = sign::listen(token, wait).unwrap() else {
        panic!("UART IRQ missing")
    };
    irq_received();
    sequence
}
fn deadline() -> Wait {
    Wait::At(room::clock().saturating_add(2_000_000_000))
}
fn accord(root: PendId, task: TaskId) -> PendId {
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Task(task),
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::Sign(
                SignRights::LISTEN | SignRights::CLAIM | SignRights::YIELD | SignRights::REARM,
            ),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            range: PendRange::Inherit,
        },
    }
    .to_bytes();
    pend::accord(super::ptr(&bytes)).unwrap()
}
extern "C" fn dying_driver() -> ! {
    let token = PendId::mint(TOKEN.load(Ordering::Acquire));
    let generation = sign::claim(token).unwrap();
    sign::rearm(token, generation, 0).unwrap();
    write(1, 2);
    let sequence = receive(token, deadline());
    GENERATION.store(generation as usize, Ordering::Release);
    SEQUENCE.store(sequence as usize, Ordering::Release);
    // Exit with a device still asserting and a controller claim still held.
    unit::exit(0, 0, VirtAddr::new(0), 0)
}
extern "C" fn revoked_driver() -> ! {
    let token = PendId::mint(TOKEN.load(Ordering::Acquire));
    let generation = sign::claim(token).unwrap();
    sign::rearm(token, generation, 0).unwrap();
    ENTERED.store(1, Ordering::Release);
    assert!(sign::listen(token, deadline()).is_err());
    unit::exit(0, 0, VirtAddr::new(0), 0)
}
pub(super) fn run(at: usize, count: usize) {
    assert_eq!(count, args::LEN);
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let ledger = Entries::new(unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    })
    .unwrap();
    let root = PendId::mint(
        ledger
            .find(Name::Interrupt {
                controller: 0xc000000,
                source: 10,
            })
            .expect("UART interrupt grant")
            .handle(),
    );
    let page = PendId::mint(
        ledger
            .find(Name::Page(Page::Region(0x10000000)))
            .unwrap()
            .handle(),
    );
    let (address, _) = share_view::open(page).unwrap();
    UART.store(address.get(), Ordering::Release);
    write(1, 0);
    assert_ne!(read(5) & 0x20, 0);
    assert!(sign::listen(root, Wait::Poll).is_err());
    let generation = sign::claim(root).unwrap();
    assert!(sign::claim(root).is_err());
    assert!(sign::trigger(root).is_err());
    let unrelated = sign::create(crate::license::get()).unwrap();
    sign::rearm(root, generation, 0).unwrap();
    let mut previous = 0;
    for turn in 0..24 {
        write(1, 2);
        if turn == 0 {
            room::sleep_until(room::clock() + 2_000_000).unwrap();
        }
        let sequence = receive(root, if turn == 0 { Wait::Poll } else { deadline() });
        assert!(sequence > previous);
        assert_eq!(
            sign::listen(root, Wait::Poll).unwrap(),
            SignReply::Triggered { sequence }
        );
        assert_eq!(
            sign::listen(unrelated, Wait::Poll).unwrap(),
            SignReply::Pending
        );
        assert_eq!(read(2) & 0x0f, 2, "hardware THRE cause");
        write(1, 0);
        assert!(sign::consume(root, sequence).is_err());
        assert!(sign::rearm(root, generation, previous).is_err());
        sign::rearm(root, generation, sequence).unwrap();
        assert!(sign::rearm(root, generation, sequence).is_err());
        previous = sequence;
    }
    sign::yield_binding(root, generation).unwrap();
    sign::delete(unrelated).unwrap();
    pend::return_(unrelated).unwrap();
    let task = super::lifecycle::spawn(dying_driver as *const () as usize);
    let branch = accord(root, task);
    TOKEN.store(branch.get(), Ordering::Release);
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(super::lifecycle::wait_task(task));
    super::lifecycle::delete(task);
    write(1, 0);
    let replacement = sign::claim(root).unwrap();
    assert!(replacement > GENERATION.load(Ordering::Acquire) as u64);
    assert!(
        sign::rearm(
            root,
            GENERATION.load(Ordering::Acquire) as u64,
            SEQUENCE.load(Ordering::Acquire) as u64
        )
        .is_err()
    );
    sign::rearm(root, replacement, 0).unwrap();
    write(1, 2);
    let sequence = receive(root, deadline());
    write(1, 0);
    sign::rearm(root, replacement, sequence).unwrap();
    sign::yield_binding(root, replacement).unwrap();
    let task = super::lifecycle::spawn(revoked_driver as *const () as usize);
    let branch = accord(root, task);
    TOKEN.store(branch.get(), Ordering::Release);
    unit::resume(UnitTarget::Task(task)).unwrap();
    let until = room::clock() + 2_000_000_000;
    while ENTERED.load(Ordering::Acquire) == 0 {
        assert!(room::clock() < until);
        room::yield_();
    }
    room::sleep_until(room::clock() + 2_000_000).unwrap();
    pend::revoke(task, branch).unwrap();
    assert!(super::lifecycle::wait_task(task));
    super::lifecycle::delete(task);
    let generation = sign::claim(root).unwrap();
    sign::rearm(root, generation, 0).unwrap();
    write(1, 2);
    let sequence = receive(root, deadline());
    write(1, 0);
    sign::rearm(root, generation, sequence).unwrap();
    sign::yield_binding(root, generation).unwrap();
    share_view::shut(page).unwrap();
    writeln!(
        super::Console,
        "PASS interrupt uart=1 retained=1 repeated=24 isolated=1 stale=1 death=1 revoke=1 rebind=1"
    )
    .unwrap();
}
