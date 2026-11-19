#![no_std]
#![no_main]
extern crate alloc;
#[path = "../../pinned/platform/device.rs"]
mod device;
#[path = "../../pinned/platform/window.rs"]
mod window;
mod uart_boot;
// Reuse immutable production firmware parsers for this test-only boot path.
use platform_riscv::{harts, memory, resources, tree};
use kernel::entry::boot::{
    self,
    memory::topology::{Fabric, Ram, Topology},
};
#[unsafe(no_mangle)]
extern "C" fn main(hart: usize, dtp: usize) -> ! {
    let result = unsafe { uart_boot::init(hart, dtp) }.and_then(|_| {
        let tree = unsafe { fdt::Fdt::from_ptr(dtp as *const u8) }.unwrap();
        assert!(
            tree.root()
                .compatible()
                .all()
                .any(|v| matches!(v, "riscv-virtio,qemu" | "riscv-virtio"))
        );
        let external = device::prepare(&tree);
        // SAFETY: this emulated UART is reserved for this image's initial Team.
        let uart = kernel_api::boot::platform::Region::new(0x1000_0000, 4096);
        unsafe { boot::grant_mmio(uart) }.unwrap();
        unsafe { boot::map_entry_mmio(uart, kernel_api::address::VirtAddr::new(0x1000_0000)) }.unwrap();
        let route = kernel::entry::boot::memory::topology::Route {
            controller: external.controller as u64,
            requester: external.requester,
        };
        unsafe { boot::bind_external(external) }.unwrap();
        let regions: alloc::vec::Vec<_> = tree.memory().regions().collect();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].starting_address as usize, 0x80000000);
        assert_eq!(regions[0].size, Some(128 * 1024 * 1024));
        let bytes = unsafe { core::slice::from_raw_parts(dtp as *const u8, tree.total_size()) };
        let admitted = platform_riscv::memory::parse(bytes, dtp, boot::image_region()).unwrap();
        let ram = admitted
            .identity()
            .iter()
            .map(|r| Ram {
                base: r.base as u64,
                bytes: r.size as u64,
            })
            .collect();
        // SAFETY: this test image runs only under the paired QEMU invocation:
        // one emulated Hart, ordinary RAM, one fixed virtio-blk, no passthrough or hotplug.
        // The sole Hart executes the Kernel's FENCE.I on executable Publish/Protect.
        // Kernel independently rejects SMP and intervals outside admitted RAM.
        let topology = unsafe {
            Topology::new(alloc::vec![Fabric {
                ram,
                routes: alloc::vec![route],
            }])
        }
        .unwrap();
        boot::qualify_memory(topology).unwrap();
        boot::start()
    });
    match result {
        Ok(()) => boot::run(),
        Err(e) => boot::fail(e),
    }
}
