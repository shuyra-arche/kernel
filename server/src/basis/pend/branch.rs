use alloc::sync::Arc;
use alloc::sync::Weak;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;

use super::PendEntry;
use super::ledger::{insert_heir, with_pair_uncommitted};
use crate::basis::flat::sharing::SharingOperation;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::task::Task;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::model::pend::{Limits, PendRange};

/// Resolve the window while the source/target transaction locks are held.
fn effective_window(
    pend_entry: &PendEntry,
    requested: super::Window,
) -> Result<Option<(PageOffset, PageCount)>, PendFail> {
    if matches!(
        requested,
        super::Window::Parent(kernel_api::model::pend::PendRange::Inherit)
    ) {
        return Ok(pend_entry.authority().window());
    }
    let meta = pend_entry.target().memory().ok_or(PendFail::Denied)?;
    let outer = pend_entry
        .authority()
        .window()
        .unwrap_or((PageOffset::ZERO, meta.source().page_count()));
    requested.resolve(outer).map(Some)
}

/// Local derivation and cross-Task delegation share the same authorization commit.
pub(crate) fn accord(
    caller: &Arc<Task>,
    src: PendId,
    dst: &Weak<Task>,
    limits: Limits,
    mark: Mark,
    requested: super::Window,
    permit: Option<(PendId, kernel_api::model::pend::Operations)>,
) -> Result<usize, PendFail> {
    let share = permit.is_some_and(|(_, operation)| {
        operation
            == kernel_api::model::pend::Operations::None(
                kernel_api::model::pend::CreationRights::SHARE,
            )
    }) && matches!(
        limits.operations,
        kernel_api::model::pend::Operations::Share(_)
    );
    let target = dst.upgrade().ok_or(PendFail::Denied)?;
    if !crate::basis::unit::control::adjacent(caller, &target) {
        return Err(PendFail::Denied);
    }
    let observed = super::locate(caller, src).ok_or(PendFail::Denied)?;
    if !derivable(&observed, limits, share) {
        return Err(PendFail::Denied);
    }
    let share_view = observed.target().memory();
    let _operation = if let Some(meta) = &share_view {
        let operation = SharingOperation::acquire(meta).ok_or(PendFail::Busy)?;
        Some(operation)
    } else {
        None
    };
    let check_source = |window: Option<(PageOffset, PageCount)>| {
        if let Some(meta) = &share_view
            && let Some(authorization) = meta.authorization()
        {
            let (offset, count) = window.unwrap_or((PageOffset::ZERO, meta.source().page_count()));
            authorization.check_publish(offset, count)?;
        }
        Ok::<(), PendFail>(())
    };
    let token = with_pair_uncommitted(caller, &target, || {
        let _commit = crate::basis::commit();
        if !super::live(caller) || !super::live(&target) {
            return Err(PendFail::Dead);
        }
        if let Some((token, operation)) = permit {
            if !super::locate(caller, token).is_some_and(|entry| entry.permits(operation)) {
                return Err(PendFail::Denied);
            }
        }
        let pend_entry = super::locate(caller, src).ok_or(PendFail::Denied)?;
        if !pend_entry.alive() {
            return Err(PendFail::Dead);
        }
        if !derivable(&pend_entry, limits, share) {
            return Err(PendFail::Denied);
        }
        let mark = if mark == Mark::NONE {
            pend_entry.mark()
        } else {
            mark
        };
        if share_view.is_some() && pend_entry.authority().modifiable().is_none() {
            return Err(PendFail::Busy);
        }
        let window = effective_window(&pend_entry, requested)?;
        check_source(window)?;
        let mut granted = pend_entry.vest_window(mark, limits, window, share)?;
        granted.parent(src, Arc::downgrade(caller));
        let granted = Arc::try_new(granted).map_err(|_| PendFail::OutOfMemory)?;
        {
            let mut heirs = caller.pend.heirs.lock();
            if heirs.len() >= 1024 {
                return Err(PendFail::OutOfMemory);
            }
            heirs.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        }
        let registration = target.pend.prepare(granted.clone())?;
        let token = granted.token();
        crate::basis::room::interrupt::dispatch(&target, &granted)?;
        insert_heir(caller, src, Arc::downgrade(&target), token);
        registration.publish();
        Ok(token)
    })??;
    let _ = sign::wake(
        WakeKey::Pends {
            task: target.ident.id,
        },
        &target.life(),
    );
    super::notify(caller.ident.id, src);
    super::notify(target.ident.id, token);
    Ok(token.get())
}

/// Narrow preserves identity. Live descendants/external loans return Busy before
/// changing the window; the holder can Return/Revoke them and retry.
pub(crate) fn reduce(task: &Arc<Task>, token: PendId, limits: Limits) -> Result<(), PendFail> {
    let range = limits.range;
    let observed = super::locate(task, token).ok_or(PendFail::Denied)?;
    let share = observed.target().memory();
    let _operation = share
        .as_ref()
        .map(|meta| SharingOperation::acquire(meta).ok_or(PendFail::Busy))
        .transpose()?;
    let _serial = task.pend.lock();
    let observed = super::locate(task, token).ok_or(PendFail::Denied)?;
    if !observed.alive() {
        return Err(PendFail::Dead);
    }
    observed.authority().can_narrow(limits)?;
    if task
        .pend
        .heirs
        .lock()
        .iter()
        .any(|(parent, _, _)| *parent == token)
    {
        return Err(PendFail::Busy);
    }
    let window = match range {
        PendRange::Inherit => observed.authority().window(),
        _ => {
            let meta = share.as_ref().ok_or(PendFail::Denied)?;
            let outer = observed.authority().window().unwrap_or((
                kernel_api::address::PageOffset::ZERO,
                meta.source().page_count(),
            ));
            Some(super::Window::Parent(range).resolve(outer)?)
        }
    };
    if let (Some(meta), Some((offset, count))) = (&share, window) {
        meta.source()
            .range(offset, count)
            .map_err(|_| PendFail::Invalid)?;
    }
    let authorization = share.as_ref().and_then(|meta| meta.authorization());
    // No in-flight publication may cross the narrowing commit.
    {
        let _commit = crate::basis::commit();
        let _withdraw = authorization
            .as_ref()
            .map(|source| source.begin_withdraw())
            .transpose()?;
    }
    if let Some(meta) = &share {
        if limits != observed.authority().limits() || window != observed.authority().window() {
            meta.close_maps(Some(token))?;
        }
    }
    {
        let _commit = crate::basis::commit();
        let _withdraw = authorization
            .as_ref()
            .map(|source| source.begin_withdraw())
            .transpose()?;
        let entries = task.pend.entries.lock();
        let entry = entries.get(token).ok_or(PendFail::Denied)?;
        entry.narrow(limits, window)?;
    }
    drop(_serial);
    drop(_operation);
    super::notify(task.ident.id, token);
    Ok(())
}

/// Flat → Share is an explicit attenuating creation operation, never a generic
/// Accord target change. Data operations and deletion cannot exceed the Flat.
fn derivable(entry: &PendEntry, limits: Limits, share: bool) -> bool {
    use kernel_api::model::pend::{Distributable, FlatRights, Operations, ShareRights};
    let parent = entry.authority().limits();
    if share && matches!(entry.target(), super::Target::Flat(_)) {
        let Operations::Flat(rights) = parent.operations else {
            return false;
        };
        let Operations::Share(requested) = limits.operations else {
            return false;
        };
        let mut permitted = ShareRights::empty();
        if rights.contains(FlatRights::ASSOCIATE) {
            permitted |= ShareRights::OPEN | ShareRights::SHUT | ShareRights::ASSOCIATE;
        }
        if rights.contains(FlatRights::DELETE) {
            permitted |= ShareRights::DELETE;
        }
        limits.valid()
            && parent.distributable == Distributable::Yes
            && parent.modifiable.allows(limits.modifiable)
            && rights.contains(FlatRights::SHARE)
            && permitted.contains(requested)
    } else {
        parent.permits_child(limits)
    }
}
