pub(crate) use crate::external_error::Error as PublicationError;
#[path = "../../../src/basis/nest/error.rs"]
mod error;
pub(crate) use error::NestError;
pub(crate) mod address {
    pub(crate) enum AddressError {
        OutOfMemory,
        NotAligned,
        NoRegion,
        Occupied,
    }
}
pub(crate) mod authority {
    pub(crate) struct RangeBinding;
}
mod memo;
