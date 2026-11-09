//! 在世任务名册：按 id 找得到每一枚还活着的任务。
//!
//! `insert` 在任务真正进世界那一刻登记（先留位、后插入，插不下就退回去）；
//! 最后一份强引用释放时按身份摘除登记，不扫描其他 Task。

use alloc::sync::Arc;

use hashbrown::HashMap;

use kernel_api::model::identity::TaskId;

use crate::basis::unit::task::Origin;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskWeak;
use crate::metal::sync::Level;
use crate::metal::sync::OnceLock;
use crate::metal::sync::SpinLock;

static TABLE: OnceLock<SpinLock<HashMap<TaskId, TaskWeak>>> = OnceLock::new();

fn table() -> &'static SpinLock<HashMap<TaskId, TaskWeak>> {
    TABLE.get_or_init(|| SpinLock::new_level(Level::Registry, HashMap::new()))
}

/// 先给这一枚任务留个位子：留不下就别往下构造了。
pub(crate) fn reserve() -> Result<(), ()> {
    let mut entries = table().lock();
    entries.try_reserve(1).map_err(|_| ())
}

pub(crate) fn insert<T>(commit: impl FnOnce() -> (Arc<Task>, T)) -> Result<(Arc<Task>, T), ()> {
    let mut entries = table().lock();
    if entries.len() == entries.capacity() {
        return Err(());
    }
    #[cfg(debug_assertions)]
    let _commit = crate::metal::allocator::NoAllocation::enter();
    let (task, value) = commit();
    let id = task.ident.id;
    let previous = entries.insert(
        id,
        TaskWeak::stored(Arc::downgrade(&task), Origin::Registry),
    );
    assert!(previous.is_none(), "insert: duplicate task id");
    Ok((task, value))
}

/// 按 id 取一份任务的弱引用（找不到或已死掉之外的事都不管）。
pub(crate) fn find(id: TaskId) -> Option<TaskWeak> {
    table().lock().get(&id).map(|w| w.copy_at(Origin::Lookup))
}

/// Only Task::drop removes its registration: completed retirement is not proof
/// that other in-flight owners have released their strong references. No registry
/// operation drops a strong Task reference while holding this table lock.
pub(super) fn remove(id: TaskId) {
    let removed = TABLE.get().and_then(|table| {
        let mut entries = table.lock();
        if let Some(task) = entries.get(&id) {
            assert!(
                !task.live(),
                "registration outlives every strong Task owner"
            );
        }
        entries.remove(&id)
    });
    // Dropping the last explicit Weak may release storage. Keep it outside the
    // registry guard, including the already-cleared shutdown case.
    drop(removed);
}

#[cfg(debug_assertions)]
pub(crate) fn live_ids() -> (usize, [usize; 8]) {
    let g = table().lock();
    let mut out = [0usize; 8];
    let mut n = 0usize;
    let mut more = 0usize;
    for (id, w) in g.iter() {
        if !w.live() {
            continue;
        }
        if n < 8 {
            out[n] = id.get();
            n += 1;
        } else {
            more += 1;
        }
    }
    (more, out)
}

pub(crate) fn after(mut id: usize) -> Option<Arc<Task>> {
    loop {
        let (next, task) = {
            let entries = table().lock();
            let (next, task) = entries
                .iter()
                .filter(|(key, _)| key.get() > id)
                .min_by_key(|(key, _)| key.get())?;
            (next.get(), task.as_weak().clone())
        };
        if let Some(task) = task.upgrade() {
            return Some(task);
        }
        id = next;
    }
}

pub(crate) fn clear() {
    if let Some(table) = TABLE.get() {
        table.lock().clear();
    }
}
