//! RV64/SBI boot input for the selected RISC-V hardware provider.
//! Firmware declarations are not validated hardware capabilities: Kernel checks
//! the executing hart/image and the backend probes or rejects optional features.
use super::{Memory, Region};

pub mod hart;
pub use hart::{Cpu, FirmwareId, HartId, Harts, Paging};

#[derive(Debug, Clone, Copy, Default)]
/// Firmware promises for the selected RV64 timer/counter backend. These flags
/// cannot establish ISA support or grant a Unit access to controller registers.
pub struct ClockAccess {
    pub user_time: bool,
    pub supervisor_stimecmp: bool,
}
#[derive(Debug)]
/// Complete trusted input for RV64 only. Separate references keep bounded
/// pre-heap memory and CPU admission off the same ROOT-stack frame.
pub struct Platform {
    pub hart: &'static Harts,
    pub memory: &'static Memory,
    pub clock: ClockAccess,
}
impl Platform {
    pub fn initrd(&self) -> Option<Region> {
        self.memory.initrd()
    }
    pub fn dtb(&self) -> Region {
        self.memory.metadata()
    }
}

pub mod devices;
