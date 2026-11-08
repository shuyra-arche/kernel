use alloc::sync::Arc;
use alloc::sync::Weak;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;

use super::entry::Modifiable;
use super::entry::PendSnapshot;
use crate::basis::pend::narrows;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::task::Task;
use crate::basis::view::share::ShareOperation;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;

/// Resolve the window while the source/target transaction locks are held.
fn effective_window(
    pend_entry: &PendSnapshot,
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
    subset: Modifiable,
    mark: Mark,
    requested: super::Window,
) -> Result<usize, PendFail> {
    let target = dst.upgrade().ok_or(PendFail::Denied)?;
    if !crate::basis::unit::control::adjacent(caller, &target) {
        return Err(PendFail::Denied);
    }
    let observed = super::locate(caller, src).ok_or(PendFail::Denied)?;
    if !narrows(observed.modifiable(), subset) {
        return Err(PendFail::Denied);
    }
    let share_view = observed.view().share_view();
    let _operation = if let Some(meta) = &share_view {
        let operation = ShareOperation::acquire(meta).ok_or(PendFail::Busy)?;
        Some(operation)
    } else {
        None
    };
    let token = super::with_pair_uncommitted(caller, &target, || {
        let (pend_entry, granted) = {
            let _commit = crate::basis::commit();
            if !super::live(caller) || !super::live(&target) {
                return Err(PendFail::Dead);
            }
            let pend_entry = super::locate(caller, src).ok_or(PendFail::Denied)?;
            if !pend_entry.alive() {
                return Err(PendFail::Dead);
            }
            if !narrows(pend_entry.modifiable(), subset) {
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
            let mut granted = pend_entry.vest_window(mark, subset, window)?;
            granted.parent(src, Arc::downgrade(caller));
            caller
                .pend
                .heirs
                .lock()
                .try_reserve(1)
                .map_err(|_| PendFail::OoM)?;
            target
                .pend
                .entries
                .lock()
                .try_reserve(1)
                .map_err(|_| PendFail::OoM)?;
            (pend_entry, granted)
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
        super::insert_heir(caller, src, Arc::downgrade(&target), token);
        target.pend.entries.lock().push(granted);
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
