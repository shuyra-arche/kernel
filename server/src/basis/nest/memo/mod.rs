//! Nest-owned logical positions, their selected sources and exact hardware obligations.
mod publisher;
pub(crate) use publisher::Publisher;
pub(crate) mod publication;
pub(crate) mod source;
mod table;
pub(super) use table::Memos;

mod state;
use crate::basis::nest::NestError;
use kernel_api::abi::call::{cell::CellFail, pend::PendFail};
pub(crate) use state::Memo;
pub(super) use state::Settled;

pub(super) fn namespace_error(error: PendFail) -> NestError {
    match error {
        PendFail::OutOfMemory => NestError::OutOfMemory,
        PendFail::Busy => NestError::Busy,
        PendFail::NotAligned => NestError::NotAligned,
        _ => NestError::Pinned,
    }
}

pub(super) fn source_error(error: CellFail) -> NestError {
    match error {
        CellFail::OutOfMemory => NestError::OutOfMemory,
        CellFail::Busy => NestError::Busy,
        CellFail::NotAligned => NestError::NotAligned,
        CellFail::NoRegion => NestError::NoRegion,
        CellFail::SourceEmpty => NestError::Missing,
        _ => NestError::Pinned,
    }
}

pub(super) fn publication_failure(error: super::PublicationError) -> CellFail {
    use super::PublicationError;
    match error {
        PublicationError::OutOfMemory | PublicationError::Capacity => CellFail::OutOfMemory,
        PublicationError::Overlap => CellFail::AlreadyMapped,
        PublicationError::InvalidRange => CellFail::NotAligned,
        _ => CellFail::Busy,
    }
}

fn pend_failure(error: PendFail) -> CellFail {
    match error {
        PendFail::OutOfMemory => CellFail::OutOfMemory,
        PendFail::Busy => CellFail::Busy,
        PendFail::NotAligned => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}
