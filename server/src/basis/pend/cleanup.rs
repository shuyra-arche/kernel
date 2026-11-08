use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;

use super::entry::PendEntry;
use crate::basis::unit::task::Task;
use crate::basis::view::share::ShareOperation;

pub(crate) struct Cleanup {
    removed: Vec<PendEntry>,
    tasks: Vec<TaskId>,
}

impl Cleanup {
    pub(crate) fn finish(self) -> usize {
        let Cleanup { removed, tasks } = self;
        let count = removed.len();
        for pend_entry in removed {
            settle(&tasks, pend_entry);
        }
        count
    }
}

/// 在 pend、commit 与 ShareOperation 锁外通知并释放已摘取的 PendEntry。
fn settle(tasks: &[TaskId], pend_entry: PendEntry) {
    // Publish the returned borrow before waking observers. Waking first can let
    // an observer re-park on Suspended and miss the only completion notification.
    pend_entry
        .authority()
        .settle()
        .expect("children settled before parent");
    for task in tasks {
        super::notify(*task, pend_entry.token());
    }
    if let (Some(token), Some(parent)) = (pend_entry.sire(), pend_entry.lord().upgrade()) {
        super::notify(parent.ident.id, token);
    }
    drop(pend_entry);
}

#[allow(clippy::type_complexity)]
pub(super) fn collect(
    task: &Arc<Task>,
    token: PendId,
    recursive: bool,
) -> Result<(Vec<(Arc<Task>, usize)>, Vec<(Arc<Task>, PendId)>), PendFail> {
    let mut tasks = Vec::new();
    let mut nodes = Vec::new();
    nodes.try_reserve(1).map_err(|_| PendFail::OoM)?;
    nodes.push((task.clone(), token));
    let mut cursor = 0;
    while cursor < nodes.len() {
        let (task, token) = nodes[cursor].clone();
        let parent = {
            let _pend = task.pend.lock();
            super::observe(&mut tasks, &task, task.pend.version.load(Ordering::Relaxed))?;
            let pend_entry = super::locate(&task, token).ok_or(PendFail::Busy)?;
            let parent = if cursor == 0 {
                pend_entry.lord().upgrade()
            } else {
                None
            };
            if cursor == 0 || recursive {
                let heirs = task.pend.heirs.lock();
                let start = heirs.partition_point(|(source, _, _)| source.get() < token.get());
                for (_, child, child_token) in heirs[start..]
                    .iter()
                    .take_while(|(source, _, _)| *source == token)
                {
                    if let Some(child) = child.upgrade() {
                        nodes.try_reserve(1).map_err(|_| PendFail::OoM)?;
                        nodes.push((child, *child_token));
                    }
                }
            }
            parent
        };
        if let Some(parent) = parent {
            let _pend = parent.pend.lock();
            super::observe(
                &mut tasks,
                &parent,
                parent.pend.version.load(Ordering::Relaxed),
            )?;
        }
        cursor += 1;
    }
    tasks.sort_unstable_by_key(|(task, _)| task.ident.id.get());
    Ok((tasks, nodes))
}

fn relations_match(tasks: &[(Arc<Task>, usize)], nodes: &[(Arc<Task>, PendId)]) -> bool {
    for (index, (task, token)) in nodes.iter().enumerate() {
        let Some(pend_entry) = super::locate(task, *token) else {
            return false;
        };
        if let Some(parent) = pend_entry.sire() {
            let Some(lord) = pend_entry.lord().upgrade() else {
                return false;
            };
            if index == 0 {
                if !tasks.iter().any(|(holder, _)| Arc::ptr_eq(holder, &lord)) {
                    return false;
                }
            } else if !nodes
                .iter()
                .any(|(holder, source)| *source == parent && Arc::ptr_eq(holder, &lord))
            {
                return false;
            }
            if !lord
                .pend
                .heirs
                .lock()
                .iter()
                .any(|(source, child, child_token)| {
                    *source == parent
                        && *child_token == *token
                        && child.ptr_eq(&Arc::downgrade(task))
                })
            {
                return false;
            }
        } else if index != 0 {
            return false;
        }
        let heirs = task.pend.heirs.lock();
        let start = heirs.partition_point(|(source, _, _)| source.get() < token.get());
        for (_, child, child_token) in heirs[start..]
            .iter()
            .take_while(|(source, _, _)| *source == *token)
        {
            if child.strong_count() != 0
                && !nodes.iter().skip(1).any(|(holder, token)| {
                    *token == *child_token && child.ptr_eq(&Arc::downgrade(holder))
                })
            {
                return false;
            }
        }
    }
    true
}

pub(super) fn take(task: &Task, token: PendId) -> Option<PendEntry> {
    let pend_entry = {
        let mut entries = task.pend.entries.lock();
        let at = entries.iter().position(|p| p.token() == token)?;
        entries.remove(at)
    };
    pend_entry.invalidate();
    if let (Some(parent), Some(lord)) = (pend_entry.sire(), pend_entry.lord().upgrade()) {
        super::remove_heir(&lord, parent, token);
        super::changed(&lord);
    }
    task.pend
        .heirs
        .lock()
        .retain(|(parent, _, _)| *parent != token);
    super::changed(task);
    Some(pend_entry)
}

pub(super) fn cull(
    task: &Arc<Task>,
    token: PendId,
    caller: Option<&Arc<Task>>,
    _closing: bool,
) -> Result<Cleanup, PendFail> {
    for _ in 0..super::RETRIES {
        if super::locate(task, token).is_none() {
            return Err(PendFail::Denied);
        }
        let (mut tasks, nodes) = match collect(task, token, true) {
            Err(PendFail::Busy) => continue,
            result => result?,
        };
        if let Some(caller) = caller {
            let _pend = caller.pend.lock();
            super::observe(
                &mut tasks,
                caller,
                caller.pend.version.load(Ordering::Relaxed),
            )?;
        }
        tasks.sort_unstable_by_key(|(task, _)| task.ident.id.get());
        let mut removed = Vec::new();
        let mut changed = Vec::new();
        removed
            .try_reserve(nodes.len())
            .map_err(|_| PendFail::OoM)?;
        changed
            .try_reserve(tasks.len())
            .map_err(|_| PendFail::OoM)?;
        let root = super::locate(task, token).ok_or(PendFail::Denied)?;
        let share_view = root.view().share_view();
        let _operation = match &share_view {
            Some(meta) => {
                let operation = ShareOperation::acquire(meta).ok_or(PendFail::Busy)?;
                Some(operation)
            }
            None => None,
        };
        let result = super::with_tasks_checked(
            &tasks,
            || relations_match(&tasks, &nodes),
            || {
                let root = super::locate(task, token).ok_or(PendFail::Denied)?;
                if let Some(caller) = caller {
                    let parent = root.sire().ok_or(PendFail::Denied)?;
                    let lord = root.lord().upgrade().ok_or(PendFail::Denied)?;
                    if !Arc::ptr_eq(&lord, caller) || super::locate(caller, parent).is_none() {
                        return Err(PendFail::Denied);
                    }
                }
                let authorization = share_view.as_ref().and_then(|meta| meta.authorization());
                let _publication = authorization
                    .as_ref()
                    .map(|source| source.begin_withdraw())
                    .transpose()?;
                for (holder, token) in &nodes {
                    super::locate(holder, *token)
                        .ok_or(PendFail::Busy)?
                        .authority()
                        .invalidate();
                }
                Ok(())
            },
        );
        match result {
            Err(PendFail::Busy) => {
                core::hint::spin_loop();
                continue;
            }
            result => result??,
        }
        // No task/commit locks are held while remote TLB completion is awaited.
        // Retiring Pends reject all new publications. Failure leaves them in-table
        // so the same operation can retry without returning the parent borrow.
        if let Some(meta) = &share_view {
            for (_, token) in nodes.iter().rev() {
                meta.close_maps(Some(*token))?;
            }
        }
        if nodes.iter().any(|(holder, token)| {
            super::locate(holder, *token).is_some_and(|entry| entry.authority().external_pending())
        }) {
            return Err(PendFail::Busy);
        }
        super::with_tasks_checked(
            &tasks,
            || relations_match(&tasks, &nodes),
            || {
                for (holder, token) in nodes.iter().rev() {
                    if let Some(entry) = take(holder, *token) {
                        removed.push(entry);
                    }
                }
                for (holder, _) in &tasks {
                    changed.push(holder.ident.id);
                }
            },
        )?;
        return Ok(Cleanup {
            removed,
            tasks: changed,
        });
    }
    Err(PendFail::Busy)
}

/// 同一提交边界内摘取无父、无后代且无映射的本地根；其他结构返回 None。
/// 其他 Nest 的退休票继续持帧，不以本 Share 的映射账为空证明全局结清。
fn reap(task: &Arc<Task>, token: PendId) -> Result<Option<PendEntry>, PendFail> {
    let _pend = task.pend.lock();
    let _commit = crate::basis::commit();
    let Some(pend_entry) = super::locate(task, token) else {
        return Ok(None);
    };
    if pend_entry.sire().is_some() || pend_entry.authority().external_pending() {
        return Ok(None);
    }
    if task
        .pend
        .heirs
        .lock()
        .iter()
        .any(|(source, _, _)| *source == token)
    {
        return Ok(None);
    }
    let share_view = pend_entry.view().share_view();
    let _operation = match &share_view {
        Some(meta) => {
            let operation = ShareOperation::acquire(meta).ok_or(PendFail::Busy)?;
            // ShareOperation 阻止并发发布修改映射账。
            if meta.has_maps() {
                return Ok(None);
            }
            Some(operation)
        }
        None => None,
    };
    let authorization = share_view.as_ref().and_then(|meta| meta.authorization());
    let _publication = authorization
        .as_ref()
        .map(|source| source.begin_withdraw())
        .transpose()?;
    Ok(Some(take(task, token).ok_or(PendFail::Denied)?))
}

pub(crate) fn return_(task: &Arc<Task>, token: PendId) -> Result<usize, PendFail> {
    for _ in 0..super::RETRIES {
        match reap(task, token) {
            Ok(Some(pend_entry)) => {
                settle(&[task.ident.id], pend_entry);
                return Ok(1);
            }
            Ok(None) => return cull(task, token, None, false).map(|cleanup| cleanup.finish()),
            Err(PendFail::Busy) => core::hint::spin_loop(),
            Err(error) => return Err(error),
        }
    }
    Err(PendFail::Busy)
}

pub(crate) fn revoke(
    caller: &Arc<Task>,
    target: &Weak<Task>,
    token: PendId,
) -> Result<usize, PendFail> {
    let target = target.upgrade().ok_or(PendFail::Denied)?;
    cull(&target, token, Some(caller), false).map(|cleanup| cleanup.finish())
}

pub(crate) fn doom(task: &Arc<Task>) {
    crate::basis::unit::team::exit::member_dying(task, 0);
    let tokens = {
        let _pend = task.pend.lock();
        super::changed(task);
        // 这个 Task 的来源引用走到尽头：本表清空，未消费的一次性资格随最后一份引用结束。
        // 关系不由这里拆——那是来源 Pend 原有责任，走 Share / Nest 的清理链，
        // 不让这张表把同步失败吞掉。
        task.pend.references.clear();
        let mut tokens = Vec::new();
        let entries = task.pend.entries.lock();
        if tokens.try_reserve(entries.len()).is_ok() {
            tokens.extend(entries.iter().map(|p| p.token()));
        }
        tokens
    };
    // 退场只推进控制清理，不保证结清；HUSKS 的再次 retire 才负责重试未结项。
    let _ = task.close_controls();
    seal_owned(task);
    for token in tokens {
        if let Ok(cleanup) = cull(task, token, None, true) {
            cleanup.finish();
        }
    }
}

/// Bounded cleanup continuation retained by Kernel after execution has ended.
pub(crate) fn retire(task: &Arc<Task>) -> Result<(), PendFail> {
    for _ in 0..16 {
        let token = task.pend.entries.lock().first().map(|entry| entry.token());
        let Some(token) = token else {
            return Ok(());
        };
        cull(task, token, None, true)?.finish();
    }
    Err(PendFail::Busy)
}

fn seal_owned(task: &Task) {
    if task.ident.id.get() == 0 {
        return;
    }
    let mut after = 0;
    loop {
        let pend_entry = {
            let _pend = task.pend.lock();
            task.pend
                .entries
                .lock()
                .iter()
                .filter(|pend_entry| pend_entry.view().owner() == task.ident.id)
                .filter(|pend_entry| pend_entry.token().get() > after)
                .min_by_key(|pend_entry| pend_entry.token().get())
                .map(|p| p.snapshot())
        };
        let Some(pend_entry) = pend_entry else {
            break;
        };
        after = pend_entry.token().get();
        pend_entry.view().seal();
    }
}
