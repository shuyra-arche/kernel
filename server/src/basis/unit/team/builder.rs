use super::MAX_CHILDREN;
use super::MAX_DEPTH;
use super::lifecycle::{ExitPolicy, TeamStage, TeamState};
use super::membership::Tasks;
use crate::basis::commit;
use crate::basis::unit::life::Life;
use crate::basis::unit::life::TeamLife;
use crate::basis::unit::task::Origin;
use crate::basis::unit::task::Task;
use crate::machine::sync::Level;
use crate::machine::sync::OnceLock;
use crate::machine::sync::SpinLock;
use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use kernel_api::model::identity::TaskId;
use kernel_api::model::identity::TeamId;

use crate::basis::nest::NestError;
use crate::basis::nest::SpaceKind;
use crate::basis::unit::space::Space;
use crate::basis::unit::space::SpaceBuilder;
use crate::basis::unit::task::TaskWeak;

use super::Team;

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

static NEXT: AtomicUsize = AtomicUsize::new(1);

/// 下一个团队号：到 `isize::MAX` 就不再发新号（既有回绕口径）。
pub(crate) fn next_id() -> Result<TeamId, NestError> {
    let id = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            (id < isize::MAX as usize).then_some(id + 1)
        })
        .map_err(|_| NestError::OutOfMemory)?;
    Ok(TeamId::new(id))
}

/// 从零落一座团队：造状态机、成员表与生命周期线，再把自己的根挂到家长名下。
/// 家长位的深度与容量都在这里把关（失败时团队本身还没任何人看得到）。
pub(crate) fn make_team(
    space: Arc<Space>,
    sire: Option<Arc<Task>>,
    creator: TaskId,
    constructing: bool,
) -> Result<Arc<Team>, NestError> {
    let parent = sire.as_ref().map(|task| task.ident.team.life.clone());
    let owner = sire.as_ref().map_or(TaskId::new(0), |task| task.ident.id);
    let id = next_id()?;
    let state = Arc::try_new(SpinLock::new_level(
        Level::UnitState,
        TeamState::new(if constructing {
            TeamStage::Constructing
        } else {
            TeamStage::Ready { default_entry: 0 }
        }),
    ))
    .map_err(|_| NestError::OutOfMemory)?;
    let tasks = Arc::try_new(SpinLock::new_level(Level::TeamTasks, Vec::new()))
        .map_err(|_| NestError::OutOfMemory)?;
    let life = Arc::try_new(TeamLife {
        id,
        owner,
        parent: parent.clone(),
        state: state.clone(),
        tasks: tasks.clone(),
        children: SpinLock::new(Vec::new()),
        life: Life::try_new().map_err(|_| NestError::OutOfMemory)?,
        exit: ExitPolicy::new(),
    })
    .map_err(|_| NestError::OutOfMemory)?;
    let team = Arc::try_new(Team {
        space,
        id,
        creator,
        cells: crate::basis::cell::page::Owner::team(id).map_err(|_| NestError::OutOfMemory)?,
        sire: sire.as_ref().map_or_else(TaskWeak::empty, |task| {
            TaskWeak::stored(Arc::downgrade(task), Origin::Sire)
        }),
        state,
        tasks: Tasks {
            members: tasks,
            held: SpinLock::new_level(Level::L3, Vec::new()),
        },
        life: life.clone(),
        operation: SpinLock::new(()),
    })
    .map_err(|_| NestError::OutOfMemory)?;
    if let Some(sire) = sire {
        let parent = &sire.ident.team.life;
        {
            let mut children = parent.children.lock();
            if children.len() >= MAX_CHILDREN {
                return Err(NestError::OutOfMemory);
            }
            children
                .try_reserve(1)
                .map_err(|_| NestError::OutOfMemory)?;
        }
        sire.heir
            .lock()
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        let _commit = commit();
        if sire.dead() || !parent.admitting() {
            return Err(NestError::NoRegion);
        }
        let mut depth = 1;
        let mut ancestor = parent.parent.as_deref();
        while let Some(node) = ancestor {
            depth += 1;
            ancestor = node.parent.as_deref();
        }
        if depth >= MAX_DEPTH {
            return Err(NestError::NoRegion);
        }
        let mut children = parent.children.lock();
        let mut heirs = sire.heir.lock();
        if children.len() >= MAX_CHILDREN || children.len() == children.capacity() {
            return Err(NestError::OutOfMemory);
        }
        if heirs.len() == heirs.capacity() {
            return Err(NestError::OutOfMemory);
        }
        children.push(life);
        heirs.push(team.clone());
    }
    Ok(team)
}

pub(crate) static KERNEL_TEAM: OnceLock<Arc<Team>> = OnceLock::new();

/// 落座内核团队（只一次）：它没有家长、也没有 sire。
pub(crate) fn init_kernel(space: Arc<Space>) -> Result<&'static Arc<Team>, NestError> {
    if KERNEL_TEAM.get().is_none() {
        let team = make_team(
            space,
            None,
            kernel_api::model::identity::TaskId::new(0),
            false,
        )?;
        assert!(KERNEL_TEAM.set(team).is_ok());
    }
    Ok(KERNEL_TEAM.get().expect("kernel team initialized"))
}

pub fn kernel() -> Option<&'static Arc<Team>> {
    KERNEL_TEAM.get()
}
