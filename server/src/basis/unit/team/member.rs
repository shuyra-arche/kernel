use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::model::identity::TaskId;
use kernel_api::model::unit::TaskExit;

use crate::basis::unit::life::Life;
use crate::basis::unit::life::TeamLife;
use crate::basis::unit::task::TaskState;
use crate::basis::unit::task::TaskWeak;
use crate::machine::sync::SpinLock;

use super::Receipt;

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
    pub node: alloc::sync::Weak<TeamLife>,
    pub state: Arc<SpinLock<TaskState>>,
    pub life: Arc<Life>,
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
        let mut receipts = self.receipts.lock();
        if let Some(at) = receipts.iter().position(|r| r.owner == owner) {
            receipts.swap_remove(at);
            true
        } else {
            false
        }
    }
}
