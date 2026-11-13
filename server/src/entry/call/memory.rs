//! Shared caller-memory boundary and v10 error translation for Flat/Nest adapters.
use crate::basis::{
    flat::{FlatError, transfer::TransferError},
    nest::{Nest, NestError, Span},
    sign::{self, WakeKey},
    unit::task::{Task, TaskIdent},
};
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use kernel_api::{abi::call::cell::CellFail, address::VirtAddr as KVirt};
pub(super) fn read_snapshot<const N: usize>(space: &Nest, at: KVirt) -> Result<[u8; N], CellFail> {
    let mut bytes = [0; N];
    if !space.copy_in(&mut bytes, at) {
        return Err(CellFail::Denied);
    }
    Ok(bytes)
}

pub(super) fn caller(ident: &Arc<TaskIdent>) -> Option<Arc<Task>> {
    use crate::basis::room::scheduler::slot::current;
    use crate::basis::unit::task::registry::find;
    current()
        .running_task()
        .or_else(|| find(ident.id).and_then(|task| task.upgrade()))
}

pub(super) fn source_fail(error: kernel_api::abi::call::pend::PendFail) -> CellFail {
    use kernel_api::abi::call::pend::PendFail;
    match error {
        PendFail::OutOfMemory => CellFail::OutOfMemory,
        PendFail::Busy => CellFail::Busy,
        PendFail::NotAligned => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}

pub(super) fn wake(affected: Vec<Weak<Span>>) {
    for relation in affected {
        let Some(mapping) = relation.upgrade() else {
            continue;
        };
        let key = mapping.wake_key();
        drop(mapping);
        if let Some((space, mapping)) = key {
            sign::signal(WakeKey::Reservation {
                space,
                reservation: mapping,
            });
        }
    }
}

impl From<NestError> for CellFail {
    fn from(error: NestError) -> Self {
        match error {
            NestError::OutOfMemory => CellFail::OutOfMemory,
            NestError::NotAligned => CellFail::NotAligned,
            NestError::NoRegion | NestError::Missing => CellFail::NoRegion,
            NestError::AlreadyMapped => CellFail::AlreadyMapped,
            NestError::WidenDenied => CellFail::WidenDenied,
            NestError::Shootdown | NestError::Busy => CellFail::Busy,
            NestError::Pinned | NestError::NotMapped | NestError::DramOverlap => CellFail::Denied,
        }
    }
}

impl From<FlatError> for CellFail {
    fn from(error: FlatError) -> Self {
        NestError::from(error).into()
    }
}

impl From<TransferError> for CellFail {
    fn from(error: TransferError) -> Self {
        match error {
            TransferError::OutOfMemory => CellFail::OutOfMemory,
            TransferError::OutOfRange => CellFail::NoRegion,
            TransferError::SourceEmpty => CellFail::SourceEmpty,
            TransferError::DestinationOccupied => CellFail::DestinationOccupied,
            TransferError::NotTransferable => CellFail::NotTransferable,
            TransferError::Busy => CellFail::Busy,
        }
    }
}
