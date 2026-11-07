//! A shared ownership epoch, not a second physical resource registry.
//! Closing a Team transfers its remaining Page responsibilities to Kernel.
use alloc::alloc::AllocError;
use alloc::sync::Arc;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use kernel_api::model::cell::PageOwner;
use kernel_api::model::identity::TeamId;

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
    pub(crate) fn identity(&self) -> PageOwner {
        match self {
            Self::Team(team) if !team.closed.load(Ordering::Acquire) => PageOwner::Team(team.id),
            _ => PageOwner::Kernel,
        }
    }
    pub(crate) fn close(&self) {
        if let Self::Team(team) = self {
            team.closed.store(true, Ordering::Release);
        }
    }
}
