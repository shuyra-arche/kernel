use fack::prelude::Error;

#[derive(Error, Debug)]
pub enum MachineError {
    #[error("invalid device tree: {0}")]
    DeviceTree(fdt::FdtError),
    #[error("invalid boot memory: {0}")]
    Memory(super::memory::MemoryError),
    #[error("invalid CPU facts: {0}")]
    Harts(super::harts::HartError),
}
