use crate::machine::hart::{self, HartId};
use sbi::{IpiCall, ecall::SArgs, fid::Ipi};

/// Order the local clear before the caller inspects its work queues. A sender
/// racing that snapshot either publishes visible work or leaves an interrupt.
pub(crate) fn acknowledge() {
    unsafe { core::arch::asm!("csrci sip, 2", "fence o, rw", options(nostack)) };
}

/// Publish shared work before requesting an interrupt on the destination Hart.
pub(crate) fn send(destination: HartId) -> sbi::ecall::SResult<usize> {
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
