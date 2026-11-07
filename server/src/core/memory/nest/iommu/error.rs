use crate::core::memory::cell::RetainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HardwareError {
    Unavailable,
    QueueFull,
    Fault,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    Closed,
    UnknownBinding,
    InvalidState,
    WrongBatch,
    InvalidRange,
    Overlap,
    Capacity,
    UnknownDomain,
    OutOfMemory,
    Exhausted,
    Retain(RetainError),
    Hardware(HardwareError),
}

impl From<RetainError> for Error {
    fn from(error: RetainError) -> Self {
        Self::Retain(error)
    }
}

impl From<HardwareError> for Error {
    fn from(error: HardwareError) -> Self {
        Self::Hardware(error)
    }
}
