use alloc::sync::{Arc, Weak};

use super::Task;

#[cfg(debug_assertions)]
use core::sync::atomic::AtomicUsize;
#[cfg(debug_assertions)]
use core::sync::atomic::Ordering::Acquire;
#[cfg(debug_assertions)]
use core::sync::atomic::Ordering::Relaxed;
#[cfg(debug_assertions)]
use core::sync::atomic::Ordering::Release;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Origin {
    Registry,
    Holder,
    TeamTasks,
    Sire,
    Lookup,
}

#[cfg(debug_assertions)]
const SLOTS: usize = 48;

#[cfg(debug_assertions)]
static RECORDS: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];

#[cfg(debug_assertions)]
const ORIGINS: usize = Origin::Lookup as usize + 1;

#[cfg(debug_assertions)]
pub(crate) fn register(origin: Origin) -> usize {
    let hart = crate::metal::hart::hart_id().get();
    let record = hart * ORIGINS + origin as usize + 1;
    for (i, slot) in RECORDS.iter().enumerate() {
        if slot.compare_exchange(0, record, Acquire, Relaxed).is_ok() {
            return i + 1;
        }
    }
    0
}

#[cfg(debug_assertions)]
pub(crate) fn release(slot: usize) {
    if slot == 0 {
        return;
    }
    RECORDS[slot - 1].store(0, Release);
}

#[cfg(debug_assertions)]
pub(crate) fn check_block_heldout() {
    let me = crate::metal::hart::hart_id().get();
    for slot in &RECORDS {
        let record = slot.load(Acquire);
        if record == 0 || (record - 1) / ORIGINS != me {
            continue;
        }
        let origin = match (record - 1) % ORIGINS {
            value if value == Origin::Lookup as usize => Origin::Lookup,
            _ => continue,
        };
        panic!(
            "[weak] 挂起自检：本核栈上仍有抄件（出身：{origin:?}）—— 跨挂起的弱引用会让外壳 \
             永远归还不掉（`strong 0 weak 1`）"
        );
    }
}

/// 一个弱抄件：团队成员表、名册、等待表、家长位只挂它，不给任务强引用。调试档下
/// 每个抄件还登记一个槽位，供「跨挂起仍有抄件」自检。
pub(crate) struct TaskWeak {
    w: Weak<Task>,
    #[cfg(debug_assertions)]
    slot: usize,
}

impl TaskWeak {
    pub(crate) fn stored(w: Weak<Task>, origin: Origin) -> TaskWeak {
        #[cfg(not(debug_assertions))]
        let _ = origin;
        TaskWeak {
            w,
            #[cfg(debug_assertions)]
            slot: register(origin),
        }
    }

    pub(crate) fn live(&self) -> bool {
        self.w.strong_count() > 0
    }

    pub(crate) fn as_weak(&self) -> &Weak<Task> {
        &self.w
    }

    pub(crate) fn upgrade(&self) -> Option<Arc<Task>> {
        self.w.upgrade()
    }

    pub(crate) fn copy_at(&self, origin: Origin) -> TaskWeak {
        TaskWeak::stored(self.w.clone(), origin)
    }

    pub(crate) fn empty() -> TaskWeak {
        TaskWeak {
            w: Weak::new(),
            #[cfg(debug_assertions)]
            slot: 0,
        }
    }
}

impl Drop for TaskWeak {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        release(self.slot);
    }
}
