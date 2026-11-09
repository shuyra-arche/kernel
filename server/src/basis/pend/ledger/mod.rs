//! Task-local authority records and the ordered locking used across ledgers.

mod cleanup;
mod entry;
mod table;
pub(crate) use table::Table;

use alloc::sync::{Arc, Weak};

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::PendId;

use crate::basis::unit::task::Task;

use super::{HoldTable, live};
pub(crate) use cleanup::{doom, retire, return_, revoke};
pub(crate) use entry::{PendEntry, accede, locate, vestor};

const RETRIES: usize = 8;

pub(super) fn with_pair<T>(a: &Task, b: &Task, run: impl FnOnce() -> T) -> Result<T, PendFail> {
    with_pair_uncommitted(a, b, || {
        let _commit = crate::basis::commit();
        Ok(run())
    })?
}

pub(super) fn with_pair_uncommitted<T>(
    a: &Task,
    b: &Task,
    run: impl FnOnce() -> T,
) -> Result<T, PendFail> {
    let (first, second) = if a.ident.id.get() <= b.ident.id.get() {
        (a, b)
    } else {
        (b, a)
    };
    let _first = first.pend.try_lock().ok_or(PendFail::Busy)?;
    let _second = if first.ident.id == second.ident.id {
        None
    } else {
        Some(second.pend.try_lock().ok_or(PendFail::Busy)?)
    };
    Ok(run())
}

pub(crate) fn available(pend_entry: &PendEntry) -> Result<(), PendFail> {
    if pend_entry.authority().modifiable().is_none() {
        return Err(PendFail::HandedOver);
    }
    Ok(())
}

pub(super) fn insert_heir(task: &Task, parent: PendId, child: Weak<Task>, token: PendId) {
    let mut heirs = task.pend.heirs.lock();
    let at = heirs.partition_point(|(source, _, _)| source.get() <= parent.get());
    heirs.insert(at, (parent, child, token));
}

pub(super) fn remove_heir(task: &Task, parent: PendId, token: PendId) {
    let mut heirs = task.pend.heirs.lock();
    let start = heirs.partition_point(|(source, _, _)| source.get() < parent.get());
    if let Some(at) = heirs[start..]
        .iter()
        .take_while(|(source, _, _)| *source == parent)
        .position(|(_, _, child)| *child == token)
    {
        heirs.remove(start + at);
    }
}

pub(crate) fn insert(task: &Task, pend_entry: PendEntry) -> Result<(), PendFail> {
    let _closed = task.pend.lock();
    let _commit = crate::basis::commit();
    if !live(task) {
        return Err(PendFail::Dead);
    }
    let pend_entry = Arc::try_new(pend_entry).map_err(|_| PendFail::OutOfMemory)?;
    task.pend.prepare(pend_entry)?.publish();

    Ok(())
}

pub(crate) fn notify(task: kernel_api::model::identity::TaskId, token: PendId) {
    let _ = crate::basis::sign::signal(crate::basis::sign::WakeKey::Inspect {
        task,
        token: token.get(),
    });
}
