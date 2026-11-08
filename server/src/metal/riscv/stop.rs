use crate::metal::hart;
use crate::metal::riscv::sbi;
use ::core::sync::atomic::AtomicUsize;
use ::core::sync::atomic::Ordering;
use sbi::{Hsm, SystemReset};

static ALARMER: AtomicUsize = AtomicUsize::new(usize::MAX);
static QUIET: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn hush() {
    let owner = ALARMER.load(Ordering::Acquire);
    if owner != usize::MAX && owner != hart::hart_id().get() {
        hunker();
    }
}

fn hunker() -> ! {
    crate::metal::mmu::identity::vacate();
    unsafe { ::core::arch::asm!("csrci sstatus, 2") };
    // This path never returns: each peer acknowledges once after vacating its lease.
    QUIET.fetch_add(1, Ordering::Release);
    let _ = sbi::HsmCall::new(Hsm::Stop).call();
    loop {
        ::core::hint::spin_loop();
    }
}

pub(crate) fn claim() -> bool {
    let me = hart::hart_id().get();
    match ALARMER.compare_exchange(usize::MAX, me, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => true,
        Err(owner) if owner == me => false,
        Err(_) => hunker(),
    }
}

pub(crate) fn quiesced_peers() -> usize {
    QUIET.load(Ordering::Acquire)
}

pub(crate) fn alarm() -> bool {
    let won = claim();
    if won {
        broadcast();
    }
    won
}

pub(crate) fn broadcast() {
    let me = hart::hart_id();
    for slot in (0..hart::hart_count()).map(hart::HartId::new) {
        if slot == me {
            continue;
        }
        let _ = crate::metal::interrupt::ipi::send(slot);
    }
}

pub(crate) fn reset() {
    let _ = sbi::SystemResetCall::new(SystemReset::SystemReset).call();
}

/// Terminal failure reset; this path never returns to the caller's stack.
pub(crate) fn failed() -> ! {
    unsafe { core::arch::asm!("csrci sstatus, 2") };
    loop {
        let _ = sbi::SystemResetCall::new(SystemReset::SystemReset)
            .args(sbi::ecall::SArgs {
                a0: 0,
                a1: 1,
                ..Default::default()
            })
            .call();
        unsafe { core::arch::asm!("wfi") };
    }
}

use super::image::ROOT_STACK_SIZE;
use core::panic::PanicInfo;
static mut SCENE: [usize; 2] = [0; 2];

pub(crate) fn scene() -> (usize, usize) {
    let s = unsafe { ::core::ptr::read_volatile(::core::ptr::addr_of!(SCENE)) };
    (s[0], s[1])
}

#[allow(improper_ctypes_definitions)]
#[unsafe(naked)]
pub(crate) extern "C" fn home(_info: &PanicInfo) -> ! {
    ::core::arch::naked_asm!(
        "la   t0, {scene}",
        "sd   sp, 0(t0)",
        "sd   s0, 8(t0)",
        "la   t0, _kernel_edge",
        "li   t1, {size}",
        "add  t1, t0, t1",
        "mv   t2, sp",
        "bltu t2, t0, 1f",
        "bltu t2, t1, 2f",
        "1:  mv   sp, t1",
        "2:  tail {work}",
        scene = sym SCENE,
        size = const ROOT_STACK_SIZE,
        work = sym crate::entry::report::halt::info,
    );
}

pub(crate) fn stop_boot() {
    unsafe { ::core::arch::asm!("csrci sstatus, 2") };
    if claim() && crate::entry::boot::facts::dram_edge().is_some() {
        broadcast();
    }
}

/// Fences other harts, then parks this one. `ALARM` is a hart boundary, not
/// evidence that a device stopped.
pub(crate) fn protected_stop(_reason: usize) -> ! {
    if !alarm() {
        crate::metal::stop::hush();
    }
    unsafe { ::core::arch::asm!("csrci sstatus, 2") };
    let expected = hart::hart_count().saturating_sub(1);
    while crate::metal::stop::quiesced_peers() != expected {
        ::core::hint::spin_loop();
    }
    loop {
        unsafe { ::core::arch::asm!("wfi") };
    }
}

pub(crate) fn trap_scene() -> (usize, usize) {
    (
        riscv::register::sepc::read(),
        riscv::register::stval::read(),
    )
}
