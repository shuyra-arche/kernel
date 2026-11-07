//! Registers used by the initial, polling-only command engine (IOMMU v1.0.1 §5).
pub(crate) const CAP: usize = 0;
pub(crate) const FCTL: usize = 8;
pub(crate) const DDTP: usize = 0x10;
pub(crate) const CQB: usize = 0x18;
pub(crate) const CQH: usize = 0x20;
pub(crate) const CQT: usize = 0x24;
pub(crate) const FQB: usize = 0x28;
pub(crate) const FQH: usize = 0x30;
pub(crate) const CQCSR: usize = 0x48;
pub(crate) const FQCSR: usize = 0x4c;
pub(crate) const PQCSR: usize = 0x50;
pub(crate) const ENABLE: u32 = 1;
pub(crate) const ON: u32 = 1 << 16;
pub(crate) const BUSY: u32 = 1 << 17;
pub(crate) const CQ_ERRORS: u32 = (1 << 8) | (1 << 9) | (1 << 10);
pub(crate) const FQ_ERRORS: u32 = (1 << 8) | (1 << 9);

/// Exclusive register access and coherent CPU/IOMMU memory ordering.
///
/// # Safety
/// Implementations must access one exclusively controlled IOMMU, preserve exact
/// access widths, and make coherent Cell memory and MMIO effects ordered by sync.
/// No other CPU/Unit may modify the registers or queue memory while owned here.
/// The device must honor the v1.0.1 queue completion/off guarantees. A noncoherent
/// platform requires cache maintenance and cannot use a fence-only implementation.
pub(crate) unsafe trait Registers {
    fn read32(&self, offset: usize) -> u32;
    fn read64(&self, offset: usize) -> u64;
    fn write32(&mut self, offset: usize, value: u32);
    fn write64(&mut self, offset: usize, value: u64);
    fn sync(&self);
}
