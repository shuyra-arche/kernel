//! Logical closure is local; postorder withdrawal advances a bounded leaf batch.
//! The root stays registered on Busy until all descendant obligations settle.
use super::entry::PendEntry;
use crate::basis::{flat::sharing::SharingOperation, unit::task::Task};
use alloc::sync::{Arc, Weak};
use kernel_api::{
    abi::call::{cell::CellFail, pend::PendFail},
    model::{
        identity::PendId,
        pend::{NestScope, PendScope},
    },
};

/// Only the leaf's ledger and its immediate parent's ledger participate. A
/// failed try acquires no responsibility and the retained root can be retried.
fn with_parent<T>(
    task: &Arc<Task>,
    entry: &PendEntry,
    run: impl FnOnce() -> Result<T, PendFail>,
) -> Result<T, PendFail> {
    if let Some(parent) = entry.lord().upgrade() {
        super::with_pair(task, &parent, run)?
    } else {
        let _serial = task.pend.try_lock().ok_or(PendFail::Busy)?;
        let _commit = crate::basis::commit();
        run()
    }
}

/// The immutable authority ancestry rejects descendants as soon as this root
/// closes. Hardware accesses retain their original withdrawal obligations.
fn close(
    task: &Arc<Task>,
    token: PendId,
    caller: Option<&Arc<Task>>,
) -> Result<Arc<PendEntry>, PendFail> {
    let _serial = task.pend.try_lock().ok_or(PendFail::Busy)?;
    let _commit = crate::basis::commit();
    let root = super::locate(task, token).ok_or(PendFail::Denied)?;
    if let Some(caller) = caller {
        let source = root.sire().ok_or(PendFail::Denied)?;
        if !root.lord().ptr_eq(&Arc::downgrade(caller)) || super::locate(caller, source).is_none() {
            return Err(PendFail::Denied);
        }
    }
    if let Some(meta) = root.target().memory() {
        let authorization = meta.authorization();
        let _withdraw = authorization
            .as_ref()
            .map(|source| source.begin_withdraw())
            .transpose()?;
        root.invalidate();
    } else {
        root.invalidate();
    }
    Ok(root)
}

/// Select a leaf with at most the admitted 64 authority levels. No subtree
/// snapshot, global ledger lock set or recursive stack is constructed.
fn leaf(task: &Arc<Task>, token: PendId) -> Result<(Arc<Task>, Arc<PendEntry>), PendFail> {
    let mut holder = task.clone();
    let mut token = token;
    for _ in 0..64 {
        let next = {
            let _serial = holder.pend.try_lock().ok_or(PendFail::Busy)?;
            let _commit = crate::basis::commit();
            let entry = super::locate(&holder, token).ok_or(PendFail::Busy)?;
            entry.invalidate();
            let mut heirs = holder.pend.heirs.lock();
            let at = heirs.partition_point(|(source, _, _)| source.get() < token.get());
            match heirs.get(at).filter(|(source, _, _)| *source == token) {
                Some((_, child, child_token)) => match child.upgrade() {
                    Some(child) => Some((child, *child_token)),
                    None => {
                        // One stale edge per turn. Never spin on an orphan.
                        heirs.remove(at);
                        return Err(PendFail::Busy);
                    }
                },
                None => return Ok((holder.clone(), entry)),
            }
        };
        if let Some((child, child_token)) = next {
            holder = child;
            token = child_token;
        }
    }
    Err(PendFail::Busy)
}

fn remove(task: &Arc<Task>, entry: &Arc<PendEntry>) -> Result<(), PendFail> {
    with_parent(task, entry, || {
        let current = super::locate(task, entry.token()).ok_or(PendFail::Busy)?;
        if !Arc::ptr_eq(&current, entry) || entry.authority().external_pending() {
            return Err(PendFail::Busy);
        }
        let heirs = task.pend.heirs.lock();
        let at = heirs.partition_point(|(source, _, _)| source.get() < entry.token().get());
        if heirs
            .get(at)
            .is_some_and(|(source, _, _)| *source == entry.token())
        {
            return Err(PendFail::Busy);
        }
        drop(heirs);
        entry.authority().settle()?;
        if let (Some(source), Some(parent)) = (entry.sire(), entry.lord().upgrade()) {
            super::remove_heir(&parent, source, entry.token());
        }
        task.pend
            .entries
            .lock()
            .remove(entry.token())
            .ok_or(PendFail::Busy)?;
        Ok(())
    })?;
    entry.release();
    if let crate::basis::pend::Target::Nest { nest, .. } = entry.target()
        && let PendScope::Nest(NestScope::Reservation { id, .. }) = entry.scope()
    {
        nest.retire_reservation(id.get());
    }
    super::notify(task.ident.id, entry.token());
    if let (Some(source), Some(parent)) = (entry.sire(), entry.lord().upgrade()) {
        super::notify(parent.ident.id, source);
    }
    Ok(())
}

fn cull(task: &Arc<Task>, token: PendId, caller: Option<&Arc<Task>>) -> Result<(), PendFail> {
    let observed = super::locate(task, token).ok_or(PendFail::Denied)?;
    let memory = observed.target().memory();
    let _operation = memory
        .as_ref()
        .map(|meta| SharingOperation::acquire(meta).ok_or(PendFail::Busy))
        .transpose()?;
    let root = close(task, token, caller)?;
    for _ in 0..8 {
        let (holder, entry) = leaf(task, token)?;
        // No ledger or COMMIT guard crosses hardware withdrawal. Busy retains
        // both the closed authority and its exact mapping and hardware obligations.
        if let Some(meta) = entry.target().memory() {
            meta.close_maps(Some(entry.token()))?;
            crate::basis::nest::authority::withdraw_source_branch(&meta, entry.token(), true)?;
            crate::basis::flat::authority::retire_block(&meta, entry.authority())?;
        }
        if let crate::basis::pend::Target::Nest { nest, .. } = entry.target() {
            match entry.scope() {
                PendScope::Nest(NestScope::Reservation { id, .. }) => {
                    if nest.reservation_root(id, entry.authority()) {
                        nest.release_reservation(id).map_err(reservation_failure)?;
                    } else {
                        let window = entry.authority().window().ok_or(PendFail::Denied)?;
                        nest.withdraw_reservation(id, window, entry.authority())?;
                    }
                }
                PendScope::Nest(NestScope::Whole)
                    if entry.sire().is_none() && !nest.is_adopted() && !nest.is_external() =>
                {
                    nest.retire().map_err(|_| PendFail::Busy)?;
                }
                _ => {}
            }
        }
        remove(&holder, &entry)?;
        if Arc::ptr_eq(&root, &entry) {
            return Ok(());
        }
    }
    Err(PendFail::Busy)
}

fn reservation_failure(error: CellFail) -> PendFail {
    match error {
        CellFail::Busy => PendFail::Busy,
        CellFail::OutOfMemory => PendFail::OutOfMemory,
        _ => PendFail::Denied,
    }
}

pub(crate) fn return_(task: &Arc<Task>, token: PendId) -> Result<(), PendFail> {
    for _ in 0..super::RETRIES {
        match cull(task, token, None) {
            Err(PendFail::Busy) => core::hint::spin_loop(),
            result => return result,
        }
    }
    Err(PendFail::Busy)
}
pub(crate) fn revoke(
    caller: &Arc<Task>,
    target: &Weak<Task>,
    token: PendId,
) -> Result<(), PendFail> {
    let target = target.upgrade().ok_or(PendFail::Denied)?;
    for _ in 0..super::RETRIES {
        match cull(&target, token, Some(caller)) {
            Err(PendFail::Busy) => core::hint::spin_loop(),
            result => return result,
        }
    }
    Err(PendFail::Busy)
}

/// Closing the one ledger gate invalidates all hosted branches and their
/// descendants immediately; withdrawal remains owned by the retained husk.
pub(crate) fn doom(task: &Arc<Task>) {
    let _commit = crate::basis::commit();
    task.pend.close();
}

pub(crate) fn retire(task: &Arc<Task>) -> Result<(), PendFail> {
    let entry = task.pend.entries.lock().iter().next().cloned();
    let Some(entry) = entry else {
        return if task.pend.references.retire(task)? {
            Ok(())
        } else {
            Err(PendFail::Busy)
        };
    };
    if task.ident.id.get() != 0
        && entry.target().owner() == task.ident.id
        && !matches!(entry.target(), crate::basis::pend::Target::Nest { .. })
    {
        entry.target().seal()?;
    }
    cull(task, entry.token(), None)?;
    if task.pend.entries.lock().len() == 0 {
        if task.pend.references.retire(task)? {
            Ok(())
        } else {
            Err(PendFail::Busy)
        }
    } else {
        Err(PendFail::Busy)
    }
}
