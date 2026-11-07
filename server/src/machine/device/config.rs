use crate::core::work::device::pci::Config;

pub(crate) struct Ecam {
    base: usize,
}
impl Ecam {
    /// # Safety
    /// A fixed PCI function's 4 KiB ECAM page mapped only into Kernel. The caller
    /// owns config serialization and preserves this mapping for the full lifetime.
    pub(crate) unsafe fn new(base: usize) -> Self {
        Self { base }
    }
}
pub(super) fn order() {
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) };
}
unsafe impl Config for Ecam {
    fn read32(&self, offset: usize) -> u32 {
        assert!(offset <= 4092 && offset.is_multiple_of(4));
        order();
        let value = unsafe { ((self.base + offset) as *const u32).read_volatile() };
        order();
        u32::from_le(value)
    }
    fn write16(&mut self, offset: usize, value: u16) {
        assert!(offset <= 4094 && offset.is_multiple_of(2));
        order();
        unsafe { ((self.base + offset) as *mut u16).write_volatile(value.to_le()) };
        order();
    }
    fn write32(&mut self, offset: usize, value: u32) {
        assert!(offset <= 4092 && offset.is_multiple_of(4));
        order();
        unsafe { ((self.base + offset) as *mut u32).write_volatile(value.to_le()) };
        order();
    }
    fn sync(&self) {
        order();
    }
}
