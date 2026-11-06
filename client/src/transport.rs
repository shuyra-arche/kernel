//! Architecture transport for the typed Client effects.

/// Execute the Kernel ABI: a7 holds the slot, a0..a5 hold arguments, and
/// a0..a2 return the result. Both user and supervisor tasks use `ebreak`;
/// supervisor `ecall` is reserved for SBI.
///
/// The out-of-line register boundary preserves the existing production
/// `#[inline(never)]` invariant.
///
/// # Safety
/// The caller must run in a Kernel task context and provide the slot and
/// arguments required by that ABI operation.
#[cfg(target_arch = "riscv64")]
#[inline(never)]
pub unsafe fn trap(slot: usize, args: [usize; 6]) -> (usize, usize, usize) {
    let (v0, v1, v2);
    unsafe {
        core::arch::asm!(
            "ebreak",
            inlateout("a0") args[0] => v0,
            inlateout("a1") args[1] => v1,
            inlateout("a7") slot => _,
            inlateout("a2") args[2] => v2,
            inlateout("a3") args[3] => _,
            inlateout("a4") args[4] => _,
            inlateout("a5") args[5] => _,
            options(nostack),
        );
    }
    (v0, v1, v2)
}

/// Host builds have no native trap. Typed-call fixtures provide an explicit
/// test transport rather than silently returning a fabricated native result.
#[cfg(not(target_arch = "riscv64"))]
pub unsafe fn trap(_slot: usize, _args: [usize; 6]) -> (usize, usize, usize) {
    unimplemented!(
        "kernel_api::abi::operation::trap 只在 riscv64 上有实现（宿主侧测试不应触发真实调用）"
    )
}
