use alloc::sync::{Arc, Weak};

use kernel_api::{abi::call::pend::PendFail, model::identity::Mark, model::identity::PendId};

use super::pend_entry::{Modifiable, PendSnapshot};
use crate::basis::memory::address::{PageCount, PageOffset};
use crate::basis::work::pend::authority::narrows;
use crate::basis::work::sign::{self, WakeKey};
use crate::basis::work::unit::task::Task;
use crate::basis::work::view::share_view::ShareOperation;

/// 这一枚子分支的有效窗口。
///
/// 不请求就**继承**父的那一段（带窗口的父派生出来的子仍带那一段，不是整段来源）；
/// 请求的那一段必须落在父的有效窗口里（父没有窗口就是整段来源），越窗与溢出一律拒。
/// ABI 解码检查非空与溢出；这一层检查父范围。
/// 窗口只在 Share（有真实来源）上成立。
fn effective_window(
    pend_entry: &PendSnapshot,
    requested: Option<(PageOffset, PageCount)>,
) -> Result<Option<(PageOffset, PageCount)>, PendFail> {
    let Some(requested) = requested else {
        return Ok(pend_entry.authority().window());
    };
    let meta = pend_entry.view().share_view().ok_or(PendFail::Denied)?;
    let outer = pend_entry
        .authority()
        .window()
        .unwrap_or((PageOffset::ZERO, meta.source().page_count()));
    let outer_end = outer.0.get().checked_add(outer.1.get());
    let inner_end = requested.0.get().checked_add(requested.1.get());
    let inside = matches!(
        (outer_end, inner_end),
        (Some(outer_end), Some(inner_end))
            if requested.0.get() >= outer.0.get() && inner_end <= outer_end
    );
    if !inside {
        return Err(PendFail::Denied);
    }
    Ok(Some(requested))
}

pub(crate) fn accord(
    caller: &Arc<Task>,
    src: PendId,
    dst: &Weak<Task>,
    subset: Modifiable,
    mark: Mark,
) -> Result<usize, PendFail> {
    publish(caller, src, dst, subset, mark, None)
}

/// 本地派生：目标就是 `caller`，范围继承或收窄，仍走同一条授权发布实现。
pub(crate) fn derive(
    caller: &Arc<Task>,
    src: PendId,
    subset: Modifiable,
    mark: Mark,
    window: Option<(PageOffset, PageCount)>,
) -> Result<usize, PendFail> {
    publish(caller, src, &Arc::downgrade(caller), subset, mark, window)
}

/// 两条授权路径的唯一发布后端：`accord` 继承父窗口，`derive` 可显式收窄。
///
/// 请求的那一段用**相对来源起点的绝对页偏移**说（不是相对父分支的偏移），
/// 且必须落在父的有效窗口里：重复与嵌套派生只能单调收窄，没有放宽的路。
fn publish(
    caller: &Arc<Task>,
    src: PendId,
    dst: &Weak<Task>,
    subset: Modifiable,
    mark: Mark,
    requested: Option<(PageOffset, PageCount)>,
) -> Result<usize, PendFail> {
    let target = dst.upgrade().ok_or(PendFail::Denied)?;
    if !crate::basis::work::unit::control::adjacent(caller, &target) {
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
            let _commit = crate::basis::work::commit();
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
                authorization.check_publish(offset, count, None)?;
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
        let _commit = crate::basis::work::commit();
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
