use alloc::sync::Arc;
use alloc::sync::Weak;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;

use super::Modifiable;
use super::PendEntry;
use super::ledger::{insert_heir, with_pair_uncommitted};
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::task::Task;
use crate::basis::view::share::ShareOperation;
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
    let meta = pend_entry.view().share_view().ok_or(PendFail::Denied)?;
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
) -> Result<usize, PendFail> {
    let subset = limits.modifiable;
    let target = dst.upgrade().ok_or(PendFail::Denied)?;
    if !crate::basis::unit::control::adjacent(caller, &target) {
        return Err(PendFail::Denied);
    }
    let observed = super::locate(caller, src).ok_or(PendFail::Denied)?;
    if !observed.authority().limits().permits_child(limits) {
        return Err(PendFail::Denied);
    }
    let share_view = observed.view().share_view();
    let _operation = if let Some(meta) = &share_view {
        let operation = ShareOperation::acquire(meta).ok_or(PendFail::Busy)?;
        Some(operation)
    } else {
        None
    };
    let token = with_pair_uncommitted(caller, &target, || {
        let (pend_entry, granted, registration) = {
            let _commit = crate::basis::commit();
            if !super::live(caller) || !super::live(&target) {
                return Err(PendFail::Dead);
            }
            let pend_entry = super::locate(caller, src).ok_or(PendFail::Denied)?;
            if !pend_entry.alive() {
                return Err(PendFail::Dead);
            }
            if !pend_entry.authority().limits().permits_child(limits) {
                return Err(PendFail::Denied);
            }
            let mark = if mark == Mark::NONE {
                pend_entry.mark()
            } else {
                mark
            };
            let window = effective_window(&pend_entry, requested)?;
            // 授权发布操作的是这一段窗口：这一段的子段或别人的独立持有仍在占用就不建
            // 新分支，也不隐式裁到剩余范围（建新授权不享受“持有人自己”那一档）。
            if let Some(meta) = &share_view
                && let Some(authorization) = meta.authorization()
            {
                let (offset, count) =
                    window.unwrap_or((PageOffset::ZERO, meta.source().page_count()));
                authorization.check_publish(offset, count)?;
            }
            if share_view.is_some() {
                pend_entry.authority().can_borrow(subset)?;
            }
            let mut granted = pend_entry.vest_window(mark, limits, window)?;
            granted.parent(src, Arc::downgrade(caller));
            let granted = Arc::try_new(granted).map_err(|_| PendFail::OutOfMemory)?;
            caller
                .pend
                .heirs
                .lock()
                .try_reserve(1)
                .map_err(|_| PendFail::OutOfMemory)?;
            let registration = target.pend.prepare(granted.clone())?;
            (pend_entry, granted, registration)
        };
        // Prepared metadata and table capacity cannot fail after withdrawal. The
        // source/recipient tables stay locked, but remote TLB completion never
        // runs under the global work commit lock.
        if let Some(meta) = &share_view
            && pend_entry.modifiable() == Modifiable::Yes
        {
            meta.close_maps(Some(src))?;
        }
        let _commit = crate::basis::commit();
        if !super::live(caller) || !super::live(&target) {
            return Err(PendFail::Dead);
        }
        let token = granted.token();
        // For Share a mutable parent loses all CPU access before lending. Readers
        // may coexist; a writer is always exclusive. Clone owns independent payloads.
        if share_view.is_some() {
            pend_entry.authority().borrow(granted.authority())?;
        }
        insert_heir(caller, src, Arc::downgrade(&target), token);
        registration.publish();
        super::changed(caller);
        if !Arc::ptr_eq(caller, &target) {
            super::changed(&target);
        }
        Ok(token)
    })?;
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
    let share = observed.view().share_view();
    let _operation = share
        .as_ref()
        .map(|meta| ShareOperation::acquire(meta).ok_or(PendFail::Busy))
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
        super::changed(task);
    }
    drop(_serial);
    drop(_operation);
    super::notify(task.ident.id, token);
    Ok(())
}
