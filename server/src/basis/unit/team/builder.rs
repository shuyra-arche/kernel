use super::MAX_CHILDREN;
use super::MAX_DEPTH;
use super::lifecycle::{ExitPolicy, TeamStage, TeamState};
use super::membership::Tasks;
use crate::basis::commit;
use crate::basis::flat::{BorrowedRegion, Flat};
use crate::basis::nest::{Access, ActiveRange, Placement};
use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::task::Origin;
use crate::basis::unit::task::Task;
use crate::basis::unit::team::TeamNode;
use crate::metal::generic::mmu::Attributes;
use crate::metal::sync::Level;
use crate::metal::sync::OnceLock;
use crate::metal::sync::SpinLock;
use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;
use kernel_api::address::{PAGE_SIZE, VirtAddr};
use kernel_api::model::identity::TaskId;
use kernel_api::model::identity::TeamId;

use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::SpaceKind;
use crate::basis::unit::task::TaskWeak;

use super::Team;

/// 按程序类别准备新团队的地址空间，家长位可选。
pub struct TeamBuilder {
    kind: SpaceKind,
    sire: TaskWeak,
    creator: kernel_api::model::identity::TaskId,
    constructing: bool,
}

impl TeamBuilder {
    pub fn new(kind: SpaceKind) -> Self {
        Self {
            kind,
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
        TeamBuilder::new(kind)
            .sire(sire)
            .creator(creator)
            .constructing()
            .spawn()
    }

    pub(crate) fn prepare_nest(kind: SpaceKind) -> Result<Arc<Nest>, NestError> {
        let nest = Nest::new(kind)?;
        seed_trampoline(&nest, &kernel().ok_or(NestError::NoRegion)?.nest)?;
        Ok(nest)
    }
    /// Caller has exclusive authority to transfer this initialized, unbound Nest.
    pub(crate) fn spawn_in(self, nest: Arc<Nest>) -> Result<Arc<Team>, NestError> {
        if nest.is_external()
            || nest.kind().is_supervisor() != self.kind.is_supervisor()
            || nest.is_closed()
        {
            return Err(NestError::NoRegion);
        }
        let sire = self.sire.upgrade().ok_or(NestError::NoRegion)?;
        make_team(nest, Some(sire), self.creator, self.constructing)
    }

    pub fn spawn(self) -> Result<Arc<Team>, NestError> {
        let sire = match self.sire.upgrade() {
            Some(task) => Some(task),
            None if self.sire.as_weak().ptr_eq(&Weak::new()) => None,
            None => return Err(NestError::NoRegion),
        };
        let nest = Self::prepare_nest(self.kind)?;
        make_team(nest, sire, self.creator, self.constructing)
    }
}

static NEXT: crate::basis::name::Serial = crate::basis::name::Serial::new(1, isize::MAX as usize);

/// 下一个团队号：到 `isize::MAX` 就不再发新号（永久耗尽，不回绕）。
pub(crate) fn next_id() -> Result<TeamId, NestError> {
    let id = NEXT.allocate().ok_or(NestError::OutOfMemory)?;
    Ok(TeamId::new(id))
}

/// 从零落一座团队：造状态机、成员表与生命周期线，再把自己的根挂到家长名下。
/// 家长位的深度与容量都在这里把关（失败时团队本身还没任何人看得到）。
pub(crate) fn make_team(
    nest: Arc<Nest>,
    sire: Option<Arc<Task>>,
    creator: TaskId,
    constructing: bool,
) -> Result<Arc<Team>, NestError> {
    let parent = sire.as_ref().map(|task| task.ident.team.node.clone());
    let owner = sire.as_ref().map_or(TaskId::new(0), |task| task.ident.id);
    let id = next_id()?;
    let state = SpinLock::new_level(
        Level::UnitState,
        TeamState::new(if constructing {
            TeamStage::Constructing
        } else {
            TeamStage::Ready { default_entry: 0 }
        }),
    );
    let tasks = SpinLock::new_level(Level::TeamTasks, Vec::new());
    let life = Arc::try_new(TeamNode {
        id,
        owner,
        parent: parent.clone(),
        state,
        tasks,
        children: SpinLock::new(Vec::new()),
        life: Anchor::try_new().map_err(|_| NestError::OutOfMemory)?,
        exit: ExitPolicy::new(),
        termination: SpinLock::new(crate::basis::unit::lifecycle::Termination::new()),
    })
    .map_err(|_| NestError::OutOfMemory)?;
    let team = Arc::try_new(Team {
        nest,
        external: crate::basis::nest::authority::admission::Admission::new(),
        id,
        creator,
        cells: crate::basis::cell::page::Owner::team(id).map_err(|_| NestError::OutOfMemory)?,
        sire: sire.as_ref().map_or_else(TaskWeak::empty, |task| {
            TaskWeak::stored(Arc::downgrade(task), Origin::Sire)
        }),
        tasks: Tasks {
            held: SpinLock::new_level(Level::L3, Vec::new()),
        },
        node: life.clone(),
        operation: SpinLock::new(()),
    })
    .map_err(|_| NestError::OutOfMemory)?;
    if let Some(sire) = sire {
        let parent = &sire.ident.team.node;
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
        let at = children.partition_point(|node| node.id.get() < life.id.get());
        children.insert(at, life);
        heirs.push(team.clone());
    }
    Ok(team)
}

pub(crate) static KERNEL_TEAM: OnceLock<Arc<Team>> = OnceLock::new();

/// 落座内核团队（只一次）：它没有家长、也没有 sire。
pub(crate) fn init_kernel(nest: Arc<Nest>) -> Result<&'static Arc<Team>, NestError> {
    if KERNEL_TEAM.get().is_none() {
        let team = make_team(
            nest,
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

/// 把内核空间的跳板映射复制进新空间：来源是内核那一段借用的物理区间。
///
/// 来源先备齐再登记占用；挂载失败的责任按 `Nest` 的划分走。
fn seed_trampoline(target: &Arc<Nest>, source: &Nest) -> Result<(), NestError> {
    const TRAMPOLINE: VirtAddr = crate::metal::layout::TRAMPOLINE;
    let (physical, flags) = source.translate(TRAMPOLINE).ok_or(NestError::NoRegion)?;
    let region = unsafe { BorrowedRegion::new(physical, PAGE_SIZE)? };
    let source_range = Flat::from_region(region)?.whole();
    let access = Access::from_attributes(flags);
    let attrs =
        flags & (Attributes::USER | Attributes::GLOBAL | Attributes::ACCESSED | Attributes::DIRTY);
    // 活动形状与来源都在登记占用之前备好：准备失败不放下一笔无人持有的占用。
    let active = ActiveRange::new(TRAMPOLINE, PAGE_SIZE, access, access)?;
    let reservation = target.reserve(Placement::fixed(TRAMPOLINE, PAGE_SIZE)?)?;
    target.attach_shaped(reservation, source_range, attrs, &[active])?;
    Ok(())
}
