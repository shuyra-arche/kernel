//! Fixed QEMU fixture topology. Device business code stays in tests/, not Kernel.
pub struct Edu {
    base: usize,
}

fn order() {
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) };
}

impl Edu {
    pub fn new(slot: usize) -> Self {
        assert!((1..=2).contains(&slot));
        let config = (0x3000_0000 + (slot << 15)) as *mut u32;
        let base = 0x4000_0000 + (slot - 1) * 0x10_0000;
        unsafe {
            assert_eq!(config.read_volatile(), 0x11e8_1234);
            // Bus mastering remains off until the retained domain is Active.
            config.add(1).cast::<u16>().write_volatile(2);
            config.add(4).write_volatile(base as u32);
        }
        order();
        let edu = Self { base };
        assert_eq!(edu.read(0) & 0xffff, 0xed);
        edu
    }

    pub fn enable(&self, slot: usize) {
        order();
        unsafe { ((0x3000_0004 + (slot << 15)) as *mut u16).write_volatile(6) };
        order();
    }

    pub fn read(&self, offset: usize) -> u32 {
        order();
        let value = unsafe { ((self.base + offset) as *const u32).read_volatile() };
        order();
        value
    }

    fn write(&self, offset: usize, value: u64) {
        order();
        unsafe { ((self.base + offset) as *mut u64).write_volatile(value) };
    }

    pub fn transfer(&self, address: usize, to_ram: bool) {
        self.write(0x80, if to_ram { 0x40000 } else { address as u64 });
        self.write(0x88, if to_ram { address as u64 } else { 0x40000 });
        self.write(0x90, 64);
        self.write(0x98, if to_ram { 3 } else { 1 });
        let start = ticks();
        loop {
            if self.read(0x98) & 1 == 0 {
                break;
            }
            // QEMU virt mtime is 10 MHz; EDU's delayed timer needs virtual time.
            assert!(ticks().wrapping_sub(start) < 20_000_000, "EDU timeout");
        }
    }

    /// Synchronous PCI memory read, not interpretation of a business result.
    pub fn drain(&self) {
        let _ = self.read(0);
        order();
    }
}

fn ticks() -> u64 {
    unsafe { (0x0200_bff8 as *const u64).read_volatile() }
}
