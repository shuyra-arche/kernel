use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::model::identity::{TaskId, TeamId};
use kernel_api::model::unit::TaskExit;

use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::task::{Task, TaskState, TaskWeak};
use crate::basis::unit::team::TeamNode;
use crate::metal::sync::SpinLock;

/// Executions retained while held or parked. Persistent membership belongs to TeamNode.
pub(crate) struct Tasks {
    pub(crate) held: SpinLock<Vec<Arc<Task>>>,
}

impl Tasks {
    /// 暂扣一个任务：同一任务只登记一次（重复停泊是幂等的），容量必须已由发布
    /// 路径留出。
    pub(crate) fn park(&self, task: Arc<Task>) {
        let mut held = self.held.lock();
        if !held.iter().any(|t| Arc::ptr_eq(t, &task)) {
            assert!(
                held.len() < held.capacity(),
                "park capacity reserved before publish"
            );
            held.push(task);
        }
    }

    /// 释放一个任务：从暂扣表里摘掉，回答是否本来就在表上。
    pub(crate) fn release_held(&self, task: &Arc<Task>) -> bool {
        let mut held = self.held.lock();
        if let Some(at) = held.iter().position(|t| Arc::ptr_eq(t, task)) {
            held.swap_remove(at);
            true
        } else {
            false
        }
    }
}

/// 一条收据：某个家长（`owner`）还在哪些子团队根（`roots`）上欠着一次退出。
pub(crate) struct Receipt {
    pub owner: TaskId,
    pub roots: Vec<TeamId>,
}

/// 团队里的一名成员：身份、外壳（弱引用）、与任务共享的状态机、生命周期线，
/// 以及它替家长欠着的收据。
pub(crate) struct Member {
    pub id: TaskId,
    pub team: kernel_api::model::identity::TeamId,
    pub controller: TaskId,
    pub creator: TaskId,
    pub sire: TaskId,
    pub reclaimed: core::sync::atomic::AtomicBool,
    pub task: TaskWeak,
    pub node: alloc::sync::Weak<TeamNode>,
    pub state: Arc<SpinLock<TaskState>>,
    pub life: Arc<Anchor>,
    pub receipts: SpinLock<Vec<Receipt>>,
    /// Explicitly returned value, retained with the completion receipts.
    pub result: SpinLock<Option<usize>>,
}

impl Member {
    pub fn exit(&self) -> Option<TaskExit> {
        let (cause, reason) = self.state.lock().exit()?;
        Some(TaskExit {
            task: self.id,
            cause,
            reason,
        })
    }

    pub fn pending(&self, owner: TaskId) -> bool {
        self.receipts.lock().iter().any(|r| r.owner == owner)
    }

    pub fn receive(&self, owner: TaskId) -> bool {
        let _commit = crate::basis::commit();
        {
            let mut receipts = self.receipts.lock();
            let Some(at) = receipts.iter().position(|r| r.owner == owner) else {
                return false;
            };
            receipts.swap_remove(at);
        }
        if let Some(node) = self.node.upgrade() {
            node.remove(self.id);
        }
        true
    }

    /// Publish cleanup completion before removing an unobserved member. The
    /// record remains if any controller still owns its immutable exit result.
    pub(crate) fn reclaim(&self) {
        let _commit = crate::basis::commit();
        self.reclaimed
            .store(true, core::sync::atomic::Ordering::Release);
        if let Some(node) = self.node.upgrade() {
            node.remove(self.id);
        }
    }

    pub(super) fn retained(&self) -> bool {
        !self.reclaimed.load(core::sync::atomic::Ordering::Acquire)
            || !self.receipts.lock().is_empty()
    }

    pub(super) fn clear(&self, owner: TaskId, root: Option<TeamId>) {
        let mut receipts = self.receipts.lock();
        for receipt in receipts.iter_mut().filter(|r| r.owner == owner) {
            if let Some(root) = root {
                receipt.roots.retain(|id| *id != root);
            } else {
                receipt.roots.clear();
            }
        }
        receipts.retain(|r| !r.roots.is_empty());
    }
}
