use alloc::sync::{Arc, Weak};

use crate::core::memory::nest::{NestError, SpaceKind};
use crate::core::work::unit::space::{Space, SpaceBuilder};
use crate::core::work::unit::task::TaskWeak;

use super::Team;
use super::construct::make_team;

/// 造一座团队：空间给定，家长位可选。
pub struct TeamBuilder {
    space: Space,
    sire: TaskWeak,
    creator: kernel_api::model::identity::TaskId,
    constructing: bool,
}

impl TeamBuilder {
    pub fn new(space: Space) -> Self {
        Self {
            space,
            sire: TaskWeak::empty(),
            creator: kernel_api::model::identity::TaskId::new(0),
            constructing: false,
        }
    }

    pub fn sire(mut self, sire: TaskWeak) -> Self {
        self.sire = sire;
        self
    }

    pub fn creator(mut self, creator: kernel_api::model::identity::TaskId) -> Self {
        self.creator = creator;
        self
    }

    pub(crate) fn constructing(mut self) -> Self {
        self.constructing = true;
        self
    }

    pub(crate) fn build_with_creator(
        kind: SpaceKind,
        sire: TaskWeak,
        creator: kernel_api::model::identity::TaskId,
    ) -> Result<Arc<Team>, NestError> {
        let space = match kind {
            SpaceKind::Supervisor => SpaceBuilder::supervisor(),
            SpaceKind::User => SpaceBuilder::user(),
        }
        .build()?;
        TeamBuilder::new(space)
            .sire(sire)
            .creator(creator)
            .constructing()
            .spawn()
    }

    pub fn spawn(self) -> Result<Arc<Team>, NestError> {
        let sire = match self.sire.upgrade() {
            Some(task) => Some(task),
            None if self.sire.as_weak().ptr_eq(&Weak::new()) => None,
            None => return Err(NestError::NoRegion),
        };
        let space = Arc::try_new(self.space).map_err(|_| NestError::OutOfMemory)?;
        make_team(space, sire, self.creator, self.constructing)
    }
}
