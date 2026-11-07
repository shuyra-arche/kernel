use crate::basis::cell::page::Owner;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::MaybeUninit;

use crate::basis::flat::Flat;
use crate::basis::nest::Access;
use crate::basis::nest::NestError;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionId;
use crate::basis::unit::life::Life;
use crate::basis::unit::space::Space;
use crate::basis::unit::team::Team;
use crate::basis::unit::team::kernel;
use crate::machine::layout::HART_FRAME_BASE;
use crate::machine::layout::IMAGE_BASE;
use crate::machine::layout::TASK_STACK_SIZE;
use crate::machine::switcher::context::TrapContext;
use crate::machine::sync::Level;
use crate::machine::sync::SpinLock;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;
use kernel_api::address::PhysAddr;
use kernel_api::address::VirtAddr;

use super::MAX_ARGS;
use super::Task;
use super::ident::TaskIdent;
use super::ident::TaskMemory;
use super::ident::next_id;
use super::prepared::PreparedTask;
use super::state::TaskState;

fn write_args(space: &Space, at: VirtAddr, args: &[usize]) {
    let mut done = 0usize;
    while done < args.len() {
        let va = at + done * size_of::<usize>();
        let pa = space
            .nest
            .physical_at(va)
            .expect("stack page materialized before args write");
        let page_rest = PAGE_SIZE - (va.as_usize() % PAGE_SIZE);
        let n = core::cmp::min((args.len() - done) * size_of::<usize>(), page_rest)
            / size_of::<usize>();
        let dst = pa.as_usize() as *mut usize;
        for i in 0..n {
            unsafe { core::ptr::write_volatile(dst.add(i), args[done + i]) };
        }
        done += n;
    }
}

/// 装配一个任务：起一个外壳、栈与参数，再交给 `PreparedTask` 等着公开。
pub struct TaskBuilder {
    team: Arc<Team>,
    controller: kernel_api::model::identity::TaskId,
    entry: VirtAddr,
    args: Vec<usize>,
    stack: usize,
}

impl TaskBuilder {
    pub fn new(team: Arc<Team>) -> TaskBuilder {
        let entry = match team.default_entry() {
            0 => IMAGE_BASE,
            e => crate::machine::mmu::riscv::canonical(e),
        };
        TaskBuilder {
            controller: team.creator,
            team,
            entry,
            args: Vec::new(),
            stack: TASK_STACK_SIZE,
        }
    }

    pub(crate) fn controller(mut self, controller: kernel_api::model::identity::TaskId) -> Self {
        self.controller = controller;
        self
    }

    pub fn args(mut self, args: Vec<usize>) -> TaskBuilder {
        debug_assert!(args.len() <= MAX_ARGS, "args 超过 MAX_ARGS");
        self.args = args;
        self
    }

    pub fn entry(mut self, entry: VirtAddr) -> TaskBuilder {
        self.entry = entry;
        self
    }

    pub fn stack(mut self, size: usize) -> TaskBuilder {
        self.stack = size;
        self
    }

    pub fn hold(self) -> Result<Arc<Task>, NestError> {
        if !self.team.ready() {
            return Err(NestError::WidenDenied);
        }
        self.prepare()?.publish(|| {})
    }

    pub(crate) fn prepare(self) -> Result<PreparedTask, NestError> {
        let stack_size = self
            .stack
            .max(1)
            .checked_next_multiple_of(PAGE_SIZE)
            .filter(|size| {
                size.checked_add(crate::machine::layout::TASK_STACK_GUARD)
                    .is_some()
            })
            .ok_or(NestError::NoRegion)?;
        let id = next_id()?;

        let mut memory = TaskMemory {
            space: self.team.space.clone(),
            stack: None,
            frame: None,
            guard: None,
        };
        let nest = &self.team.space.nest;
        // 栈主体页先备齐，再登记占用：来源准备失败不留无人持有的地址。
        let mut indexed = Vec::new();
        indexed
            .try_reserve_exact(stack_size / PAGE_SIZE)
            .map_err(|_| NestError::OutOfMemory)?;
        for page in 0..stack_size / PAGE_SIZE {
            indexed.push((
                PageIndex::new(page),
                crate::basis::flat::allocate_cell(self.team.cells.clone())?,
            ));
        }
        let count = PageCount::new(indexed.len()).ok_or(NestError::NotAligned)?;
        let source = Flat::from_cells(self.team.cells.clone(), count, indexed)?.whole();
        let slot = nest.reserve(Placement::highest(
            RegionId::Normal,
            stack_size + crate::machine::layout::TASK_STACK_GUARD,
        )?)?;
        let (guard, body) =
            nest.split_reservation(slot, crate::machine::layout::TASK_STACK_GUARD)?;
        memory.guard = Some(guard);
        let stack_body_top = body.start().as_usize() + body.size();
        let rw = Access::READ | Access::WRITE;
        memory.stack = Some(nest.attach(body, source, rw, rw)?);

        let frame = crate::basis::flat::allocate_cell(Owner::Kernel)?;
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
        let reservation =
            nest.reserve(Placement::lowest(RegionId::Kernel, PAGE_SIZE, PAGE_SIZE)?)?;
        let frame_va = reservation.start();
        memory.frame = Some(nest.attach(reservation, source, rw, rw)?);

        let count = self.args.len();
        let args_at = stack_body_top - count * size_of::<usize>();
        if count > 0 {
            write_args(
                &self.team.space,
                crate::machine::mmu::riscv::canonical(args_at),
                &self.args,
            );
        }
        let sp = VirtAddr::wrap(args_at & !0xF);

        let satp = self.team.space.nest.satp();
        let spp = self.team.space.kind().spp();
        let frame = unsafe { &mut *(frame_pa.as_usize() as *mut TrapContext) };
        unsafe {
            let ktc = kernel()
                .expect("kernel team not initialized")
                .space
                .nest
                .physical_at(HART_FRAME_BASE)
                .expect("kernel frame not mapped")
                .as_usize() as *const TrapContext;
            frame.init(
                &*ktc,
                satp,
                spp,
                self.entry,
                sp,
                (VirtAddr::wrap(args_at), count),
                frame_pa,
                frame_va,
            );
        }

        let alloc = crate::machine::allocator::hybrid::allocator();
        let ident: Arc<TaskIdent> = unsafe {
            let ident = crate::tag!(
                Task,
                Arc::try_new_in(
                    TaskIdent::new(id, self.controller, self.team.clone(), frame_pa, memory),
                    alloc,
                )
            )
            .map_err(|_| NestError::OutOfMemory)?;
            let (ptr, _alloc) = Arc::into_raw_with_allocator(ident);
            Arc::from_raw(ptr)
        };
        let life = Life::try_new().map_err(|_| NestError::OutOfMemory)?;
        let state = Arc::try_new(SpinLock::new_level(Level::UnitState, TaskState::held()))
            .map_err(|_| NestError::OutOfMemory)?;
        let slot: Arc<MaybeUninit<Task>> = unsafe {
            let slot = crate::tag!(Task, Arc::<Task, _>::try_new_uninit_in(alloc))
                .map_err(|_| NestError::OutOfMemory)?;
            let (ptr, _) = Arc::into_raw_with_allocator(slot);
            Arc::from_raw(ptr)
        };
        Ok(PreparedTask::new(ident, life, state, slot))
    }
}
