use super::memory::Memory;
use super::{MachineError, Region};
use crate::machine::hart::FirmwareId;
use crate::machine::image::root_stack_edge;

use super::harts::HartInfo;

#[derive(Debug)]
pub struct Machine {
    pub hart: &'static HartInfo,
    pub memory: Memory,
}

impl Machine {
    pub(super) fn parse(dtp: usize, boot: FirmwareId) -> Result<Self, MachineError> {
        // Firmware supplies a readable FDT blob. Validate its structure and all
        // memory facts before any iterator, allocation or secondary-hart work.
        let fdt =
            unsafe { fdt::Fdt::from_ptr(dtp as *const u8) }.map_err(MachineError::DeviceTree)?;
        let bytes = unsafe { core::slice::from_raw_parts(dtp as *const u8, fdt.total_size()) };
        unsafe extern "C" {
            static _kernel_base: u8;
        }
        let base = (&raw const _kernel_base).addr();
        let kernel = Region::new(base, root_stack_edge() - base);
        let memory = Memory::parse(bytes, dtp, kernel).map_err(MachineError::Memory)?;
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
