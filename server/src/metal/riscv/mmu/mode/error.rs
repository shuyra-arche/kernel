#[derive(fack::prelude::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatpError {
    #[error("page table mode already initialized")]
    AlreadyInit,
}
