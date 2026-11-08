//! Ordered access to an exclusively held PCI configuration function.

/// # Safety
/// One fixed function's configuration, exclusively held by Kernel. Accesses use
/// exact widths, are ordered, and remain valid until the endpoint is retired.
/// Device topology and routing cannot change behind this object. Reading and
/// writing standard config registers has the PCI-defined effects.
pub(crate) unsafe trait Config {
    fn read32(&self, offset: usize) -> u32;
    fn write16(&mut self, offset: usize, value: u16);
    fn write32(&mut self, offset: usize, value: u32);
    fn sync(&self);
}
