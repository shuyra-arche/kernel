#![no_std]
#![no_main]
extern crate alloc;
use kernel::entry::boot::{
    self,
    memory::topology::{Fabric, Ram, Topology},
};
#[unsafe(no_mangle)]
extern "C" fn main(hart: usize, dtp: usize) -> ! {
    let result = unsafe { platform_riscv::boot::init(hart, dtp) }.and_then(|_| {
        let tree = unsafe { fdt::Fdt::from_ptr(dtp as *const u8) }.unwrap();
        assert!(
            tree.root()
                .compatible()
                .all()
                .any(|v| matches!(v, "riscv-virtio,qemu" | "riscv-virtio"))
        );
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
        // SAFETY: paired QEMU virt 1/2/4 Hart CPU-only test, 128 MiB
        // coherent RAM, no executable Publish, device route or hotplug.
        assert!(matches!(tree.cpus().count(), 1 | 2 | 4));
        let topology = unsafe {
            Topology::new(alloc::vec![Fabric {
                ram,
                routes: alloc::vec![],
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
