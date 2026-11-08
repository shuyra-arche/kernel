use super::memory::Memory;
use super::{MachineError, Region};
use crate::metal::hart::FirmwareId;
use crate::metal::image::root_stack_edge;

use super::harts::HartInfo;

#[derive(Debug)]
pub struct Machine {
    pub hart: &'static HartInfo,
    pub memory: &'static Memory,
}

impl Machine {
    pub(super) fn parse(dtp: usize, boot: FirmwareId) -> Result<Self, MachineError> {
        let memory = memory(dtp)?;
        // The previous step validated this blob before any infallible iterator.
        let bytes = unsafe { core::slice::from_raw_parts(dtp as *const u8, memory.dtb().size) };
        let fdt = fdt::Fdt::new(bytes).map_err(MachineError::DeviceTree)?;
        let hart = super::admit_harts(&fdt, boot)?;
        Ok(Self { hart, memory })
    }

    pub fn initrd(&self) -> Option<Region> {
        self.memory.initrd()
    }
    pub fn dtb(&self) -> Region {
        self.memory.dtb()
    }
}

// Parse and freeze the larger memory fact set on its own call stack. Machine
// contains references only, so CPU admission does not nest beneath copies of
// the memory result. Neither fact set is exposed until Machine is installed.
fn memory(dtp: usize) -> Result<&'static Memory, MachineError> {
    static MEMORY: crate::metal::sync::OnceLock<Memory> = crate::metal::sync::OnceLock::new();
    // Firmware supplies a readable FDT blob. Validate its structure and all
    // memory facts before any iterator, allocation or secondary-hart work.
    let fdt = unsafe { fdt::Fdt::from_ptr(dtp as *const u8) }.map_err(MachineError::DeviceTree)?;
    let bytes = unsafe { core::slice::from_raw_parts(dtp as *const u8, fdt.total_size()) };
    unsafe extern "C" {
        static _kernel_base: u8;
    }
    let base = (&raw const _kernel_base).addr();
    let kernel = Region::new(base, root_stack_edge() - base);
    let memory = Memory::parse(bytes, dtp, kernel).map_err(MachineError::Memory)?;
    assert!(
        MEMORY.set(memory).is_ok(),
        "memory facts already initialized"
    );
    Ok(MEMORY.get().expect("memory facts installed"))
}
