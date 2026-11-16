//! Span-owned logical positions, their selected sources and exact hardware obligations.
mod observation;
pub(crate) use observation::{MemoInfo, MemoState};
mod error;
pub(crate) use error::AccessError;
mod publisher;
pub(crate) use publisher::Publisher;
pub(crate) mod publication;
pub(crate) mod source;
mod table;
pub(super) use table::Memos;

pub(crate) mod activity;
mod state;
use crate::basis::nest::NestError;
use kernel_api::abi::call::pend::PendFail;
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

pub(super) fn source_error(error: AccessError) -> NestError {
    match error {
        AccessError::OutOfMemory => NestError::OutOfMemory,
        AccessError::Busy => NestError::Busy,
        AccessError::NotAligned => NestError::NotAligned,
        AccessError::NoRegion => NestError::NoRegion,
        AccessError::SourceEmpty => NestError::Missing,
        _ => NestError::Pinned,
    }
}

pub(super) fn publication_failure(error: super::PublicationError) -> AccessError {
    use super::PublicationError;
    match error {
        PublicationError::OutOfMemory | PublicationError::Capacity => AccessError::OutOfMemory,
        PublicationError::Overlap => AccessError::AlreadyMapped,
        PublicationError::InvalidRange => AccessError::NotAligned,
        _ => AccessError::Busy,
    }
}

pub(super) fn pend_failure(error: PendFail) -> AccessError {
    match error {
        PendFail::OutOfMemory => AccessError::OutOfMemory,
        PendFail::Busy => AccessError::Busy,
        PendFail::NotAligned => AccessError::NotAligned,
        _ => AccessError::Denied,
    }
}
