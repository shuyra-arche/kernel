//! RISC-V execution environment for the shared synchronization protocols.
mod trap;
use crate::metal::generic::sync;
pub(crate) use trap::TrapGuard;
pub(crate) type OnceLock<T> = sync::once::OnceLock<T, Environment>;
pub(crate) struct Environment;
// SAFETY: TrapGuard masks IRQs and the reception window pins local execution;
// only the bounded, lock-free reception path runs while waiting.
unsafe impl sync::Environment for Environment {
    type Guard = TrapGuard;
    #[inline(always)]
    unsafe fn save() -> TrapGuard {
        unsafe { TrapGuard::save() }
    }
    #[inline(always)]
    fn owner() -> usize {
        crate::metal::hart::hart_id().get()
    }
    #[inline(always)]
    fn wait() {
        crate::metal::stop::hush();
        Self::poll();
        core::hint::spin_loop();
    }
    #[inline(always)]
    fn poll() {
        crate::metal::interrupt::window::poll();
    }
    #[cfg(debug_assertions)]
    #[inline(always)]
    fn caller() -> usize {
        let caller: usize;
        unsafe { core::arch::asm!("mv {0}, ra", out(reg) caller) };
        caller
    }
}
