use fack::prelude::Error;

/// 物理来源的纯域错误：不认识线上编码。
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatError {
    #[error("physical frame allocator exhausted")]
    OutOfMemory,
    #[error("physical allocation constraint is not supported")]
    NotAligned,
    #[error("page selection falls outside the source range")]
    OutOfRange,
    #[error("the page carrier has been moved out")]
    Missing,
    #[error("the page slot is held by another use")]
    Busy,
}

// Translate only at the logical source boundary; Cell retains its own errors.
impl From<crate::basis::cell::page::PageError> for FlatError {
    fn from(error: crate::basis::cell::page::PageError) -> Self {
        use crate::basis::cell::page::PageError;
        match error {
            PageError::InvalidOwner => Self::OutOfRange,
            PageError::InvalidRange => Self::NotAligned,
            PageError::IdentityExhausted | PageError::OutOfMemory => Self::OutOfMemory,
            PageError::Busy => Self::Busy,
        }
    }
}
