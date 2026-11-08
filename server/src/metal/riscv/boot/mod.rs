//! CPU-side boot handoffs. Entry owns initialization order and resource policy.

use core::arch::{asm, global_asm};
use riscv::register::satp;
use sbi::{HsmCall, ecall::SArgs, fid::Hsm};

use super::{
    hart::{self, HartId},
    mmu::mode,
    switcher::context::TrapContext,
};

global_asm!(
    ".section .text.boot",
    ".align 2",
    ".globl _boot_entry",
    "_boot_entry:",
    "    mv   tp, a1",
    "    fence r, rw",
    "    ld   t0, 0x28(tp)",
    "    bne  a0, t0, 2f",
    "    csrc sstatus, 2",
    "    ld   sp, 0x30(tp)",
    "    call boot_main",
    "2:  wfi",
    "    j 2b",
);

unsafe extern "C" {
    static _boot_entry: u8;
}

pub(crate) fn start_hart(slot: HartId, opaque: usize) -> sbi::ecall::SResult<usize> {
    unsafe { asm!("fence rw, rw", options(nostack)) };
    HsmCall::new(Hsm::Start)
        .args(SArgs {
            a0: hart::firmware(slot).get(),
            a1: (&raw const _boot_entry).addr(),
            a2: opaque,
            ..Default::default()
        })
        .call()
}

/// # Safety
/// The frame contains the initialized kernel root and the calling Hart has
/// switched to the physical bootstrap stack with interrupts masked.
pub(crate) unsafe fn activate_secondary(frame: &TrapContext) {
    let root = frame.kernel_satp;
    hart::mark_online();
    unsafe {
        satp::set(mode::mode(), root.asid(), root.ppn());
        asm!("sfence.vma", "fence.i");
    }
}

/// # Safety
/// The aligned stack belongs exclusively to this Hart and the entry never returns.
pub(crate) unsafe fn enter_idle(top: usize, entry: extern "C" fn() -> !) -> ! {
    unsafe {
        asm!(
            "mv sp, {top}",
            "jr {entry}",
            top = in(reg) top,
            entry = in(reg) entry,
            options(noreturn),
        )
    }
}
