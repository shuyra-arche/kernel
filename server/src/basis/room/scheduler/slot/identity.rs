//! Live/last execution identity and its lock-free, owner-Hart publication.
//! Only the owning Hart reads/writes this word, including its fatal trap path.
//! Publication retains the live Arc until replacement; it is an observation,
//! never a second authority for scheduling.
use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};

use kernel_api::model::identity::TaskId;
use kernel_api::model::identity::TeamId;

use crate::basis::unit::task::TaskIdent;
use kernel_api::address::PhysAddr;

use crate::basis::room::scheduler;

pub enum Identity {
    Live(Arc<TaskIdent>),
    Last { task: TaskId, team: TeamId },
}

impl Identity {
    pub fn task_id(&self) -> usize {
        match self {
            Identity::Live(t) => t.id.get(),
            Identity::Last { task, .. } => task.get(),
        }
    }

    pub fn team_id(&self) -> usize {
        match self {
            Identity::Live(t) => t.team.id.get(),
            Identity::Last { team, .. } => team.get(),
        }
    }

    pub fn trap(&self) -> Option<PhysAddr> {
        match self {
            Identity::Live(t) => Some(t.frame_pa),
            Identity::Last { .. } => None,
        }
    }

    pub fn live(&self) -> Option<&TaskIdent> {
        match self {
            Identity::Live(t) => Some(t),
            Identity::Last { .. } => None,
        }
    }
}

/// 本核在座者的身份；本核还没有运行槽时没有身份可报。
pub fn ident() -> Option<Identity> {
    if !scheduler::installed() {
        return None;
    }
    super::current().identity.read()
}

pub(super) struct Publication {
    raw: AtomicUsize,
}

impl Publication {
    const TAG: usize = 1;
    const TEAM_BITS: u32 = 31;
    const TASK_SHIFT: u32 = 1 + Self::TEAM_BITS;
    const TEAM_MASK: usize = (1 << Self::TEAM_BITS) - 1;

    pub(super) const fn new() -> Publication {
        Publication {
            raw: AtomicUsize::new(0),
        }
    }

    const fn tagged(raw: usize) -> bool {
        raw & Self::TAG != 0
    }

    const fn pack(task: TaskId, team: TeamId) -> usize {
        Self::TAG | (team.get() << 1) | (task.get() << Self::TASK_SHIFT)
    }

    const fn unpack(raw: usize) -> (TaskId, TeamId) {
        (
            TaskId::new(raw >> Self::TASK_SHIFT),
            TeamId::new((raw >> 1) & Self::TEAM_MASK),
        )
    }

    fn reclaim(raw: usize) {
        if raw != 0 && !Self::tagged(raw) {
            unsafe { drop(Arc::from_raw(raw as *const TaskIdent)) };
        }
    }

    pub(super) fn enter(&self, ident: &Arc<TaskIdent>) {
        let prev = self
            .raw
            .swap(Arc::into_raw(ident.clone()) as usize, Ordering::AcqRel);
        Self::reclaim(prev);
    }

    pub(super) fn leave(&self, ident: &TaskIdent) {
        let word = Self::pack(ident.id, ident.team.id);
        debug_assert_eq!(
            Self::unpack(word),
            (ident.id, ident.team.id),
            "末次编码往返"
        );
        let prev = self.raw.swap(word, Ordering::AcqRel);
        debug_assert!(
            prev != 0 && !Self::tagged(prev),
            "leave requires a live identity"
        );
        Self::reclaim(prev);
    }

    pub(super) fn read(&self) -> Option<Identity> {
        let raw = self.raw.load(Ordering::Acquire);
        if raw == 0 {
            None
        } else if Self::tagged(raw) {
            let (task, team) = Self::unpack(raw);
            Some(Identity::Last { task, team })
        } else {
            let live = raw as *const TaskIdent;
            unsafe {
                Arc::increment_strong_count(live);
                Some(Identity::Live(Arc::from_raw(live)))
            }
        }
    }
}
