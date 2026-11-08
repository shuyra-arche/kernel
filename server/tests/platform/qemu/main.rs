#![no_std]
#![no_main]

use core::arch::global_asm;
use kernel::entry::boot::{self, PciConfig, PciRoute, QuiescenceStatus};

global_asm!(include_str!("../../../src/metal/riscv/boot/start.S"));

const EDU: u32 = 0x11e8_1234;

#[unsafe(no_mangle)]
extern "C" fn main(_hartid: usize, dtp: usize) -> ! {
    // SAFETY: the harness starts coherent QEMU virt with one emulated EDU on
    // this IOMMU route and excludes passthrough, hotplug, ATS and bypass.
    let initialized = unsafe {
        boot::init_with_pci(
            dtp,
            PciRoute {
                host: 0x3000_0000,
                requester: 1 << 3,
                identity: EDU,
                bar: 0x4000_0000,
                quiescence,
            },
        )
    };
    match initialized.and_then(|_| boot::start()) {
        Ok(()) => boot::run(),
        Err(error) => boot::fail(error),
    }
}

fn quiescence(config: &PciConfig) -> QuiescenceStatus {
    // In this QEMU fixture, synchronous pci_dma callbacks precede the config
    // read's completion. The stopped IOMMU denies later transactions.
    if config.read32(0) == EDU {
        QuiescenceStatus::Settled
    } else {
        QuiescenceStatus::Failed
    }
}
