#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;

use kernel_std::env::{PieToken, TeamId, ViewControl, Wait};
use kernel_std::execution::memory::HeapAllocator;

// This fixture owns all three registrations. It deliberately does not depend on
// `kernel-std-rt`: the std mechanism archive must not provide `_start`,
// `save_args`, `tls_bootstrap`, `clean_ret`, a panic handler, or a global
// allocator registration.
global_asm!(
    ".section .text._start",
    ".globl _start",
    "_start:",
    "    call std_only_main",
    "1: j 1b",
);

#[global_allocator]
static HEAP: HeapAllocator = HeapAllocator;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

struct DemoMessage;

impl kernel_std::wire::Message for DemoMessage {
    type In = u8;
    type Buf = [u8; 1];
    const EMPTY: Self::Buf = [0];

    fn store(&self, out: &mut [u8]) -> Option<usize> {
        *out.first_mut()? = 1;
        Some(1)
    }

    fn fetch(bytes: &[u8]) -> Option<Self::In> {
        (bytes.len() == 1).then_some(bytes[0])
    }
}

fn dummy_entry() -> ! {
    loop {}
}

#[inline(never)]
fn exercise_mechanisms() {
    let _ = core::hint::black_box(kernel_std::execution::memory::attach_alloc(
        4096,
        ViewControl::Caller,
    ));
    let _ = core::hint::black_box(kernel_std::execution::unit::spawn(
        TeamId::new(0),
        dummy_entry as *const () as usize,
        &[],
        0,
    ));
    let _ = core::hint::black_box(kernel_std::resource::raw::inspect(PieToken::NONE));
    let _ = core::hint::black_box(kernel_std::ipc::time::deadline(Wait::POLL));
    let _ = core::hint::black_box(kernel_std::ipc::hand::Sender::<DemoMessage>::new());
    let _ = core::hint::black_box(kernel_std::ipc::hand::Receiver::<DemoMessage>::from_raw(
        PieToken::NONE,
    ));
}

#[unsafe(no_mangle)]
extern "C" fn std_only_main() -> ! {
    exercise_mechanisms();
    loop {}
}
