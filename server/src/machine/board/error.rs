use fack::prelude::Error;

#[derive(Error, Debug)]
pub enum MachineError {
    #[error("invalid device tree: {0}")]
    DeviceTree(fdt::FdtError),
    #[error("device tree has no /cpus node")]
    MissingCpus,
    #[error("invalid boot memory: {0}")]
    Memory(super::memory::MemoryError),
    #[error("invalid hart topology: count={count}, boot={boot}")]
    Harts { count: usize, boot: usize },
}
