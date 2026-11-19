//! Exact modern virtio-blk PCI composition, paired with run.py (TCG only).
use alloc::boxed::Box;
use kernel::entry::boot::{self, ExternalRoute, HardwareError, Quiescence, Retirement};
use kernel_api::{address::VirtAddr, boot::platform::Region};

const HOST: usize = 0x3000_0000;
const RID: usize = 8;
const BAR: usize = 0x4000_0000;
const SIZE: usize = 0x4000;
const ID: u32 = 0x1042_1af4;

fn order() {
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) }
}
fn read32(base: usize, offset: usize) -> u32 {
    order();
    let v = unsafe { ((base + offset) as *const u32).read_volatile() };
    order();
    v
}
fn write32(base: usize, offset: usize, value: u32) {
    order();
    unsafe { ((base + offset) as *mut u32).write_volatile(value) };
    order();
}
fn command(base: usize, value: u16) {
    order();
    unsafe { ((base + 4) as *mut u16).write_volatile(value) };
    order();
    assert_eq!(read32(base, 4) & 0x407, u32::from(value));
}

pub fn prepare(tree: &fdt::Fdt) -> ExternalRoute {
    let route = platform_riscv::topology::Topology::discover(tree)
        .unwrap()
        .resolve(HOST, RID as u16)
        .unwrap();
    let (lo, hi) = super::window::bar_window(tree, HOST).unwrap();
    assert!(lo <= BAR && BAR + SIZE <= hi);
    let config = unsafe { boot::map_mmio(HOST + RID * 4096, 4096) }.unwrap();
    assert_eq!(read32(config, 0), ID);
    command(config, 0x400);
    assert_eq!((read32(config, 12) >> 16) & 0xff, 0);
    assert!(
        matches!(read32(config, 0x100), 0 | u32::MAX),
        "extended PCI capability"
    );
    // The paired no-MSI-X, modern-only function has only BAR4 (64-bit).
    for offset in [0x10, 0x14, 0x18, 0x1c] {
        let old = read32(config, offset);
        write32(config, offset, u32::MAX);
        let mask = read32(config, offset);
        write32(config, offset, old);
        assert_eq!(mask, 0);
    }
    let old = read32(config, 0x20);
    write32(config, 0x20, u32::MAX);
    let mask = read32(config, 0x20);
    write32(config, 0x20, old);
    assert_eq!(mask & !15, !(SIZE as u32 - 1));
    assert_eq!(mask & 7, 4);
    write32(config, 0x24, 0);
    write32(config, 0x20, BAR as u32);
    assert_eq!(read32(config, 0x20) & !15, BAR as u32);
    let mut at = (read32(config, 0x34) & 0xff) as usize;
    let mut seen = [false; 64];
    let mut kinds = 0u8;
    while at != 0 {
        assert!(at >= 0x40 && at <= 0xf0 && at % 4 == 0 && !seen[at / 4]);
        seen[at / 4] = true;
        let cap = read32(config, at);
        assert_eq!(cap & 0xff, 9, "vendor virtio capability only");
        let kind = (cap >> 24) as u8;
        let len = (cap >> 16) & 255;
        assert!((1..=5).contains(&kind) && len >= 16 && at + len as usize <= 256);
        if kind <= 4 {
            assert_eq!(read32(config, at + 4) & 255, 4);
            let off = read32(config, at + 8);
            let bytes = read32(config, at + 12);
            assert!(off as usize + bytes as usize <= SIZE);
            assert_eq!(
                off,
                match kind {
                    1 => 0,
                    2 => 0x3000,
                    3 => 0x1000,
                    4 => 0x2000,
                    _ => unreachable!(),
                }
            );
            if kind == 1 {
                assert!(bytes >= 56)
            }
            if kind == 2 {
                assert!(len >= 20);
                assert_eq!(read32(config, at + 16), 4)
            }
            assert_eq!(kinds & (1 << kind), 0);
            kinds |= 1 << kind;
        }
        at = ((cap >> 8) & 255) as usize;
    }
    assert_eq!(kinds, 30);
    command(config, 0x402);
    let bar = unsafe { boot::map_mmio(BAR, SIZE) }.unwrap();
    // Static transport registers are not entered in the normal-RAM fabric.
    // Only the entry Team receives this layout; config/IOMMU remain private.
    unsafe { boot::map_entry_mmio(Region::new(BAR, SIZE), VirtAddr::new(BAR)) }.unwrap();
    command(config, 0x406);
    ExternalRoute {
        controller: route.controller(),
        requester: route.device(),
        proof: Box::new(Proof {
            controller: route.controller(),
            requester: route.device(),
            config,
            bar,
        }),
    }
}

struct Proof {
    controller: usize,
    requester: u32,
    config: usize,
    bar: usize,
}
// SAFETY: exact paired QEMU virtio-blk, no IOThread/ioeventfd/passthrough/ATS/PRI,
// one Hart and immutable route. QEMU virtio_blk_reset calls blk_drain and detaches
// queued requests before reset completes; status=0 and queue_enable=0 observed
// in a later MMIO transaction therefore follow all old callbacks. Used-ring
// completion alone is never accepted. Kernel already invalidated the leaf/DDTP
// and fenced PR/PW. A running device retains responsibility until reset. This
// one-shot binding is never reassigned while entry Team's static MMIO exists.
unsafe impl Quiescence for Proof {
    fn prove(&mut self, ticket: Retirement) -> Result<Option<Retirement>, HardwareError> {
        if ticket.controller() != self.controller
            || ticket.requester() != self.requester
            || read32(self.config, 0) != ID
        {
            return Err(HardwareError::Fault);
        }
        order();
        let status = unsafe { ((self.bar + 20) as *const u8).read_volatile() };
        let enabled = unsafe { ((self.bar + 28) as *const u16).read_volatile() };
        order();
        Ok((status == 0 && enabled == 0).then_some(ticket))
    }
}
