use crate::basis::cell::page::Owner;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::MaybeUninit;

use crate::basis::flat::Flat;
use crate::basis::nest::Access;
use crate::basis::nest::NestError;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionKind;
use crate::basis::sync::LegacyLock;
use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::team::Team;
use crate::basis::unit::team::kernel;
use crate::metal::layout::HART_FRAME_BASE;
use crate::metal::switcher::context::TrapContext;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;
use kernel_api::address::PhysAddr;

use super::Task;
use super::identity::TaskIdent;
use super::identity::TaskMemory;
use super::identity::next_id;
mod prepared;

use self::prepared::PreparedTask;
use super::TaskState;

mod bootstrap;
pub(crate) use bootstrap::{Bootstrap, BootstrapStack};
/// Prepare only a Kernel-owned execution frame and lifecycle. Ordinary user
/// memory remains owned by the caller's Flat/Nest/Memo relationships.
pub(crate) fn prepare(
    team: Arc<Team>,
    controller: kernel_api::model::identity::TaskId,
    start: kernel_api::model::unit::TaskStart,
    mut memory: TaskMemory,
) -> Result<PreparedTask, NestError> {
    let id = next_id()?;
    let nest = &team.nest;
    let rw = Access::READ | Access::WRITE;
    let frame = crate::basis::cell::allocate(Owner::Kernel)?;
    let frame_pa = PhysAddr::from_raw(frame.physical_address());
    let mut indexed = Vec::new();
    indexed
        .try_reserve_exact(1)
        .map_err(|_| NestError::OutOfMemory)?;
    indexed.push((PageIndex::ZERO, frame));
    let source = Flat::from_cells(
        Owner::Kernel,
        PageCount::new(1).expect("one frame"),
        indexed,
    )?
    .whole();
    let reservation = nest.reserve(Placement::lowest(RegionKind::Kernel, PAGE_SIZE, PAGE_SIZE)?)?;
    let frame_va = reservation.start();
    memory.frame = Some(nest.attach(reservation, source, rw, rw)?);

    let root = team.nest.root();
    let supervisor = team.nest.kind().is_supervisor();
    let frame = unsafe { &mut *(frame_pa.as_usize() as *mut TrapContext) };
    unsafe {
        let ktc = kernel()
            .expect("kernel team not initialized")
            .nest
            .physical_at(HART_FRAME_BASE)
            .expect("kernel frame not mapped")
            .as_usize() as *const TrapContext;
        frame.init(
            &*ktc,
            root,
            supervisor,
            start.entry,
            start.stack_pointer,
            (start.arguments, start.argument_count),
            frame_pa,
            frame_va,
        );
    }

    let alloc = crate::metal::allocator::hybrid::allocator();
    let ident: Arc<TaskIdent> = unsafe {
        let ident = crate::tag!(
            Task,
            Arc::try_new_in(
                TaskIdent::new(id, controller, team.clone(), frame_pa, memory),
                alloc,
            )
        )
        .map_err(|_| NestError::OutOfMemory)?;
        let (ptr, _alloc) = Arc::into_raw_with_allocator(ident);
        Arc::from_raw(ptr)
    };
    let life = Anchor::try_new().map_err(|_| NestError::OutOfMemory)?;
    let state =
        Arc::try_new(LegacyLock::new(TaskState::held())).map_err(|_| NestError::OutOfMemory)?;
    let slot: Arc<MaybeUninit<Task>> = unsafe {
        let slot = crate::tag!(Task, Arc::<Task, _>::try_new_uninit_in(alloc))
            .map_err(|_| NestError::OutOfMemory)?;
        let (ptr, _) = Arc::into_raw_with_allocator(slot);
        Arc::from_raw(ptr)
    };
    Ok(PreparedTask::new(ident, life, state, slot))
}

pub(crate) fn user(
    team: Arc<Team>,
    controller: kernel_api::model::identity::TaskId,
    start: kernel_api::model::unit::TaskStart,
) -> Result<PreparedTask, NestError> {
    let memory = TaskMemory {
        nest: team.nest.clone(),
        frame: None,
        bootstrap: None,
        start,
    };
    prepare(team, controller, start, memory)
}
