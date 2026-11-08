use crate::metal::iommu::riscv::registers::Registers;

pub(crate) struct Mmio {
    base: usize,
    physical: usize,
}

impl Mmio {
    /// # Safety
    /// `base` is a live, exclusively Kernel-controlled IOMMU MMIO page mapped
    /// with device semantics. The platform provides coherent IOMMU memory access.
    /// No other adapter or Unit may access its registers while this value lives.
    /// `physical` is the physical base of that exact mapping, not a second device.
    pub(crate) unsafe fn new(base: usize, physical: usize) -> Self {
        assert!(base != 0 && base.is_multiple_of(4096));
        assert!(physical != 0 && physical.is_multiple_of(4096));
        Self { base, physical }
    }
}

// SAFETY: construction requires exclusive coherent hardware; volatile accesses
// preserve widths and the full I/O/memory fence orders CPU and device effects.
unsafe impl Registers for Mmio {
    fn physical_base(&self) -> usize {
        self.physical
    }
    fn read32(&self, offset: usize) -> u32 {
        self.sync();
        assert!(offset <= 4092 && offset.is_multiple_of(4));
        unsafe { u32::from_le(((self.base + offset) as *const u32).read_volatile()) }
    }
    fn read64(&self, offset: usize) -> u64 {
        self.sync();
        assert!(offset <= 4088 && offset.is_multiple_of(8));
        unsafe { u64::from_le(((self.base + offset) as *const u64).read_volatile()) }
    }
    fn write32(&mut self, offset: usize, value: u32) {
        self.sync();
        assert!(offset <= 4092 && offset.is_multiple_of(4));
        unsafe { ((self.base + offset) as *mut u32).write_volatile(value.to_le()) };
    }
    fn write64(&mut self, offset: usize, value: u64) {
        self.sync();
        assert!(offset <= 4088 && offset.is_multiple_of(8));
        unsafe { ((self.base + offset) as *mut u64).write_volatile(value.to_le()) };
    }
    fn sync(&self) {
        unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) };
    }
}
