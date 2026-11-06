//! Execute Kernel operations using a selected register transport.

use env::{KernelCall, TerminalCall};

/// Pack once, invoke the supplied transport once, and decode once.
/// The callback owns its effects and safety requirements. No native trap is
/// performed here, and failure implies neither retry nor rollback.
#[inline]
pub fn invoke_with<C: KernelCall>(
    call: C,
    transport: impl FnOnce(usize, [usize; 6]) -> (usize, usize, usize),
) -> Result<C::Output, C::Error> {
    C::decode_return(transport(C::SLOT, call.pack()))
}

/// Execute an operation through the native Kernel transport.
///
/// # Safety
/// The caller must run in a Kernel task context and satisfy the selected ABI
/// operation's requirements, including pointer validity and resource lifetimes.
#[inline]
pub unsafe fn invoke<C: KernelCall>(call: C) -> Result<C::Output, C::Error> {
    invoke_with(call, |slot, args| unsafe { crate::ecall::trap(slot, args) })
}

/// Execute a terminal call, entering a non-returning fault path if it returns.
///
/// # Safety
/// The caller must satisfy the selected terminal ABI operation's context,
/// pointer and resource-lifetime requirements. No task-local cleanup follows it.
#[inline]
pub unsafe fn invoke_terminal<C: TerminalCall>(call: C) -> ! {
    unsafe {
        crate::ecall::trap(C::SLOT, call.pack());
    }
    // Cannot use TLS or enter the runtime's panic handler after termination.
    loop {
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("unimp", options(nomem, nostack));
        }
        #[cfg(not(target_arch = "riscv64"))]
        core::hint::spin_loop();
    }
}
