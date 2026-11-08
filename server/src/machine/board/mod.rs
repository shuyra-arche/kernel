mod error;
pub(crate) mod harts;
mod machine;
pub(crate) mod memory;
mod region;

pub use error::MachineError;
pub use machine::Machine;
pub use region::Region;

use crate::machine::hart;
use crate::machine::sync::OnceLock;

static MACHINE: OnceLock<Machine> = OnceLock::new();

pub fn init(dtp: usize) -> Result<(), MachineError> {
    let machine = Machine::parse(dtp, hart::firmware_id())?;
    assert!(MACHINE.set(machine).is_ok(), "machine already initialized");
    Ok(())
}

pub fn info() -> &'static Machine {
    MACHINE.get().expect("machine not initialized")
}

pub fn device_tree() -> Option<fdt::Fdt<'static>> {
    let dtb = MACHINE.get()?.dtb();
    unsafe { fdt::Fdt::from_ptr(dtb.base as *const u8) }.ok()
}

pub(crate) fn dram_edge() -> Option<usize> {
    MACHINE.get().map(|m| m.memory.envelope().range().end)
}

pub(crate) fn contains_identity(base: usize, size: usize) -> bool {
    MACHINE
        .get()
        .is_some_and(|m| m.memory.contains_identity(Region::new(base, size)))
}

// Keep the bounded CPU table out of Machine's by-value early memory result.
// Admission is single-shot and precedes allocator and secondary-hart effects.
fn admit_harts(
    tree: &fdt::Fdt,
    boot: hart::FirmwareId,
) -> Result<&'static harts::HartInfo, MachineError> {
    static HARTS: OnceLock<harts::HartInfo> = OnceLock::new();
    let facts = harts::HartInfo::parse(tree, boot).map_err(MachineError::Harts)?;
    assert!(HARTS.set(facts).is_ok(), "CPU facts already initialized");
    Ok(HARTS.get().expect("CPU facts installed"))
}
