use crate::metal::hart::{self, HartId};
use crate::metal::riscv::sbi;
use core::sync::atomic::{AtomicBool, Ordering};
use sbi::{Ipi, IpiCall, ecall::SArgs};

static WAKE: [AtomicBool; crate::metal::layout::MAX_HART_SLOTS] =
    [const { AtomicBool::new(false) }; crate::metal::layout::MAX_HART_SLOTS];

/// Order the local clear before the caller inspects its work queues. A sender
/// racing that snapshot either publishes visible work or leaves an interrupt.
pub(crate) fn acknowledge() -> bool {
    #[cfg(feature = "latency")]
    let arrived = riscv::register::sip::read().ssoft();
    unsafe { core::arch::asm!("csrci sip, 2", "fence o, rw", options(nostack)) };
    #[cfg(feature = "latency")]
    if arrived {
        crate::metal::measure::ipi_received();
    }
    // Idle consumes SSIP directly with SIE masked, without entering the trap
    // handler. Service after clearing so a racing request is observed here or
    // leaves the next interrupt pending.
    crate::metal::mmu::internal::completion::service();
    WAKE[hart::hart_id().get()].swap(false, Ordering::AcqRel)
}

/// Publish shared work before requesting an interrupt on the destination Hart.
pub(crate) fn send(destination: HartId) -> sbi::ecall::SResult<usize> {
    WAKE[destination.get()].store(true, Ordering::Release);
    send_completion(destination)
}

/// A memory rendezvous interrupts execution to fence it, but does not request
/// a scheduling transition. A simultaneous ordinary wake remains independently
/// recorded and is consumed by the same SSIP acknowledgement.
pub(crate) fn send_completion(destination: HartId) -> sbi::ecall::SResult<usize> {
    unsafe { core::arch::asm!("fence rw, ow", options(nostack)) };
    let (mask, base) = hart::firmware(destination).target();
    IpiCall::new(Ipi::SendIpi)
        .args(SArgs {
            a0: mask,
            a1: base,
            ..Default::default()
        })
        .call()
}
