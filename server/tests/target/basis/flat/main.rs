#![no_std]
#![no_main]
#[unsafe(no_mangle)]
extern "C" fn main(hart: usize, dtp: usize) -> ! {
    match unsafe { platform_riscv::boot::init(hart, dtp) }
        .and_then(|_| kernel::entry::boot::start())
    {
        Ok(()) => kernel::entry::boot::run(),
        Err(error) => kernel::entry::boot::fail(error),
    }
}
