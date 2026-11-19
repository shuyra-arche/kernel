#![no_std]
#![no_main]
mod acceptance;
mod fault;
mod support;
#[allow(dead_code)]
#[path = "../basis/room/pinned/stack.rs"]
mod stack;
mod swap;
mod smp;
mod recovery;
use core::fmt::Write;
use kernel_api::{
    abi::memory::*,
    address::*,
    model::{flat::*, unit::ExitScope},
};
use support::*;
core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail run"
);
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL native-memory {info}");
    kernel_client::call::unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
#[unsafe(no_mangle)]
extern "C" fn run(at: usize, count: usize) -> ! {
    boot(at, count);
    version_checks();
    let slot = create(1);
    assert_eq!(slot_status(slot).phase, SlotPhase::Fresh);
    mutate(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    });
    let (memo, address) = reserve(space());
    insert(memo, slot);
    publish(memo);
    unsafe {
        (address.get() as *mut u64).write_volatile(0x123456789abcdef0);
    }
    assert_eq!(
        unsafe { (address.get() as *const u64).read_volatile() },
        0x123456789abcdef0
    );
    retract(memo);
    let maintenance = block(slot);
    mutate(Action::Vacate {
        slot,
        expected: slot_status(slot).version,
        maintenance,
    });
    assert_eq!(slot_status(slot).phase, SlotPhase::Missing);
    mutate(Action::Allow { maintenance });
    let _ = writeln!(
        Console,
        "PASS native-memory abi=13 ledger=13 occupy=1 publish=1 retract=1 vacate=1"
    );
    fault::run();
    acceptance::run();
    swap::run();
    smp::run();
    kernel_client::call::unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

fn version_checks() {
    use kernel_api::abi::{self, error::DispatchFail};
    for slot in [
        (10usize << 48),
        ((abi::VERSION as usize + 1) << 48),
        abi::PREFIX | (2usize << 32),
        abi::PREFIX | (10usize << 32) | 2,
    ] {
        let result = unsafe { kernel_client::transport::trap(slot, [0; 6]) };
        assert_eq!(result.0, DispatchFail::Unknown.code() as usize);
    }
    let _ = writeln!(
        Console,
        "PASS native-version old-call=reject future-call=reject Cell=reject ShareOpen=reject"
    );
}
