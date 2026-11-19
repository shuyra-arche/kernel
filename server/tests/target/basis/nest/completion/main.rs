#![no_std]
#![no_main]

mod retirement;
#[allow(dead_code)]
mod stack;
#[allow(dead_code)]
mod support;

use core::fmt::Write;
use kernel_api::{address::VirtAddr, model::unit::ExitScope};
use support::{Console, boot};

core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail run"
);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL nest-completion {info}");
    kernel_client::call::unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

#[unsafe(no_mangle)]
extern "C" fn run(at: usize, count: usize) -> ! {
    boot(at, count);
    retirement::run();
    kernel_client::call::unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
