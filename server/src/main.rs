#![no_std]
#![no_main]

use ::core::arch::global_asm;

global_asm!(include_str!("entry/boot/start.S"));

#[unsafe(no_mangle)]
extern "C" fn main(_hartid: usize, dtp: usize) -> ! {
    match kernel::entry::boot::init(dtp).and_then(|_| kernel::entry::boot::start()) {
        Ok(()) => kernel::entry::boot::run(),
        Err(error) => kernel::entry::boot::fail(error),
    }
}
