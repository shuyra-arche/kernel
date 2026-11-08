//! Task-local authority records and the ordered locking used across ledgers.

mod cleanup;
mod entry;

use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::PendId;

use crate::basis::unit::task::Task;
use crate::machine::sync::SpinLock;

use super::{HoldTable, live};
pub(crate) use cleanup::{doom, retire, return_, revoke};
pub(crate) use entry::{PendEntry, PendSnapshot, accede, locate, vestor};

/// 一个任务的能力账本：登记表（`serial`）下挂它持有的 pend_entry、它替子任务记住的
/// 父子对（`heirs`）与来源引用（`references`）；
/// `version` 是乐观校验用的抄本号。
pub(crate) struct Table {
    serial: SpinLock<()>,
    pub(crate) version: AtomicUsize,
    pub(crate) entries: SpinLock<Vec<PendEntry>>,
    pub(crate) heirs: SpinLock<Vec<(PendId, Weak<Task>, PendId)>>,
    pub(crate) references: HoldTable,
}

impl Table {
    pub(crate) fn new() -> Self {
        Self {
            serial: SpinLock::new(()),
            version: AtomicUsize::new(0),
            entries: SpinLock::new(Vec::new()),
            heirs: SpinLock::new(Vec::new()),
            references: HoldTable::new(),
        }
    }

    pub(crate) fn lock(&self) -> crate::machine::sync::spin::SpinLockGuard<'_, ()> {
        self.serial.lock()
    }
}

const RETRIES: usize = 8;

pub(crate) fn changed(task: &Task) {
    task.pend.version.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn with_pair<T>(a: &Task, b: &Task, run: impl FnOnce() -> T) -> T {
    with_pair_uncommitted(a, b, || {
        let _commit = crate::basis::commit();
        run()
    })
}

pub(super) fn with_pair_uncommitted<T>(a: &Task, b: &Task, run: impl FnOnce() -> T) -> T {
    if a.ident.id == b.ident.id {
        let _guard = a.pend.lock();
        run()
    } else if a.ident.id.get() < b.ident.id.get() {
        let _first = a.pend.lock();
        let _second = b.pend.lock();
        run()
    } else {
        let _first = b.pend.lock();
        let _second = a.pend.lock();
        run()
    }
}

fn remember(
    tasks: &mut Vec<(Arc<Task>, usize)>,
    task: &Arc<Task>,
    version: usize,
) -> Result<(), PendFail> {
    if let Some((_, old)) = tasks.iter().find(|(old, _)| Arc::ptr_eq(old, task)) {
        return if *old == version {
            Ok(())
        } else {
            Err(PendFail::Busy)
        };
    }
    tasks.try_reserve(1).map_err(|_| PendFail::OoM)?;
    tasks.push((task.clone(), version));
    Ok(())
}

pub(super) fn observe(
    tasks: &mut Vec<(Arc<Task>, usize)>,
    task: &Arc<Task>,
    version: usize,
) -> Result<(), PendFail> {
    if tasks.iter().any(|(old, _)| Arc::ptr_eq(old, task)) {
        return Ok(());
    }
    remember(tasks, task, version)
}

pub(crate) fn available(pend_entry: &PendSnapshot) -> Result<(), PendFail> {
    if pend_entry.authority().modifiable().is_none() {
        return Err(PendFail::HandedOver);
    }
    Ok(())
}

pub(super) fn with_tasks_checked<T>(
    tasks: &[(Arc<Task>, usize)],
    validate: impl FnOnce() -> bool,
    run: impl FnOnce() -> T,
) -> Result<T, PendFail> {
    crate::machine::sync::reserve_depend(tasks.len() + 8).map_err(|_| PendFail::OoM)?;
    debug_assert!(
        tasks
            .windows(2)
            .all(|pair| pair[0].0.ident.id.get() < pair[1].0.ident.id.get())
    );
    let mut guards = Vec::new();
    guards.try_reserve(tasks.len()).map_err(|_| PendFail::OoM)?;
    for (task, _) in tasks {
        guards.push(task.pend.lock());
    }
    let commit = crate::basis::commit();
    let valid = tasks
        .iter()
        .all(|(task, version)| task.pend.version.load(Ordering::Relaxed) == *version)
        || validate();
    let result = if valid {
        Ok(run())
    } else {
        Err(PendFail::Busy)
    };
    drop(commit);
    while guards.pop().is_some() {}
    result
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
    let mut entries = task.pend.entries.lock();
    entries.try_reserve(1).map_err(|_| PendFail::OoM)?;
    entries.push(pend_entry);
    changed(task);
    Ok(())
}

pub(crate) fn notify(task: kernel_api::model::identity::TaskId, token: PendId) {
    let _ = crate::basis::sign::signal(crate::basis::sign::WakeKey::Inspect {
        task,
        token: token.get(),
    });
}
