//! Trusted Rust boot descriptors, separate from the syscall wire ABI.
//! The integration owner decodes firmware. Constructors validate bounded facts;
//! Kernel additionally checks the executing image and protected device ownership.
pub mod hart;
pub mod memory;
mod region;
pub use hart::{Cpu, FirmwareId, HartId, Harts, Paging};
pub use memory::{Memory, MemoryError};
pub use region::Region;

#[derive(Debug, Clone, Copy, Default)]
pub struct ClockAccess {
    pub user_time: bool,
    pub supervisor_stimecmp: bool,
}
#[derive(Debug)]
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
        self.memory.dtb()
    }
}

pub mod devices;
