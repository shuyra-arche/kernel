use crate::core::memory::nest::iommu::riscv::registers::Registers;

pub(crate) struct Mmio {
    base: usize,
}

impl Mmio {
    /// # Safety
    /// `base` is a live, exclusively Kernel-controlled IOMMU MMIO page mapped
    /// with device semantics. The platform provides coherent IOMMU memory access.
    /// No other adapter or Unit may access its registers while this value lives.
    pub(crate) unsafe fn new(base: usize) -> Self {
        assert!(base != 0 && base.is_multiple_of(4096));
        Self { base }
    }
}

// SAFETY: construction requires exclusive coherent hardware; volatile accesses
// preserve widths and the full I/O/memory fence orders CPU and device effects.
unsafe impl Registers for Mmio {
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
