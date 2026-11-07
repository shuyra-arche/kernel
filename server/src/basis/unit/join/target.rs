use alloc::sync::Arc;
use alloc::sync::Weak;

use crate::basis::sign::WakeKey;
use crate::basis::unit::life::Life;
use crate::basis::unit::life::TeamLife;
use crate::basis::unit::team::Member;

#[derive(Clone)]
pub(crate) enum JoinTarget {
    Task(Arc<Member>),
    Team(Arc<TeamLife>),
}

impl JoinTarget {
    pub(crate) fn key(&self) -> WakeKey {
        match self {
            Self::Task(member) => WakeKey::Task { id: member.id },
            Self::Team(team) => WakeKey::Team { id: team.id },
        }
    }

    pub(crate) fn life(&self) -> Weak<Life> {
        match self {
            Self::Task(member) => Arc::downgrade(&member.life),
            Self::Team(team) => Arc::downgrade(&team.life),
        }
    }
}
