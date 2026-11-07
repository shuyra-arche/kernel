//! A shared ownership epoch, not a second physical resource registry.
//! Closing a Team transfers its remaining Cell responsibilities to Kernel.
use alloc::{alloc::AllocError, sync::Arc};
use core::sync::atomic::{AtomicBool, Ordering};
use kernel_api::model::{identity::TeamId, memory::CellOwner};

#[derive(Clone, Debug)]
pub(crate) enum Owner {
    Kernel,
    Team(Arc<TeamOwner>),
}
#[derive(Debug)]
pub(crate) struct TeamOwner {
    id: TeamId,
    closed: AtomicBool,
}
impl Owner {
    pub(crate) fn team(id: TeamId) -> Result<Self, AllocError> {
        Ok(Self::Team(Arc::try_new(TeamOwner {
            id,
            closed: AtomicBool::new(false),
        })?))
    }
    pub(crate) fn identity(&self) -> CellOwner {
        match self {
            Self::Team(team) if !team.closed.load(Ordering::Acquire) => CellOwner::Team(team.id),
            _ => CellOwner::Kernel,
        }
    }
    pub(crate) fn close(&self) {
        if let Self::Team(team) = self {
            team.closed.store(true, Ordering::Release);
        }
    }
}
