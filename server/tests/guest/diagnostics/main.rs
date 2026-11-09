#![no_std]
#![no_main]
#[unsafe(no_mangle)]
extern "C" fn main(hart: usize, dtp: usize) -> ! {
    if cfg!(feature = "early") {
        panic!("diagnostics early before allocator initialization");
    }
    let initialized = unsafe { platform_riscv::boot::init(hart, dtp) };
    if let Err(error) = initialized.and_then(|_| kernel::entry::boot::start()) {
        kernel::entry::boot::fail(error)
    }
    #[cfg(feature = "trap")]
    unsafe {
        fault()
    }
    #[cfg(not(feature = "trap"))]
    {
        #[cfg(feature = "recursive")]
        panic!("{}", Recurse);
        #[cfg(not(feature = "recursive"))]
        panic!("diagnostics deliberate Rust panic");
    }
}
#[cfg(feature = "trap")]
#[unsafe(naked)]
unsafe extern "C" fn fault() -> ! {
    core::arch::naked_asm!(
        "li a0, 0x12345678",
        ".globl diagnostics_fault_pc",
        "diagnostics_fault_pc:",
        ".2byte 0",
        "j diagnostics_fault_pc",
    )
}
#[cfg(feature = "recursive")]
struct Recurse;
#[cfg(feature = "recursive")]
impl core::fmt::Display for Recurse {
    fn fmt(&self, _: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        panic!("recursive formatter")
    }
}
