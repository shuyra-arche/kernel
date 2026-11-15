//! Private initial-Capsule and Kernel-work stack preparation, separate from the
//! public ordinary User Task start contract.
use super::super::{MAX_ARGS, Task, identity::TaskMemory};
use super::prepared::PreparedTask;
use crate::basis::{
    flat::Flat,
    nest::{Access, Nest, NestError, Placement, RegionKind},
    unit::team::Team,
};
use crate::metal::layout::{IMAGE_BASE, TASK_STACK_SIZE};
use alloc::{sync::Arc, vec::Vec};
use kernel_api::address::{PAGE_SIZE, PageCount, PageIndex, VirtAddr};
fn write_args(space: &Nest, at: VirtAddr, args: &[usize]) {
    let mut done = 0usize;
    while done < args.len() {
        let va = at + done * size_of::<usize>();
        let pa = space
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
pub struct Bootstrap {
    team: Arc<Team>,
    controller: kernel_api::model::identity::TaskId,
    entry: VirtAddr,
    args: Vec<usize>,
    stack: usize,
}

impl Bootstrap {
    pub fn new(team: Arc<Team>) -> Bootstrap {
        let entry = match team.default_entry() {
            0 => IMAGE_BASE,
            e => crate::metal::mmu::internal::canonical(e),
        };
        Bootstrap {
            controller: team.creator,
            team,
            entry,
            args: Vec::new(),
            stack: TASK_STACK_SIZE,
        }
    }

    pub fn args(mut self, args: Vec<usize>) -> Bootstrap {
        debug_assert!(args.len() <= MAX_ARGS, "args 超过 MAX_ARGS");
        self.args = args;
        self
    }

    pub fn entry(mut self, entry: VirtAddr) -> Bootstrap {
        self.entry = entry;
        self
    }

    pub fn hold(self) -> Result<Arc<Task>, NestError> {
        if !self.team.ready() {
            return Err(NestError::WidenDenied);
        }
        self.prepare()?.publish(|| Ok(()), || {})
    }

    pub(crate) fn prepare(self) -> Result<PreparedTask, NestError> {
        let stack_size = self
            .stack
            .max(1)
            .checked_next_multiple_of(PAGE_SIZE)
            .filter(|size| {
                size.checked_add(crate::metal::layout::TASK_STACK_GUARD)
                    .is_some()
            })
            .ok_or(NestError::NoRegion)?;

        let mut stack = BootstrapStack {
            nest: self.team.nest.clone(),
            mapping: None,
            guard: None,
        };
        let nest = &self.team.nest;
        // 栈主体页先备齐，再登记占用：来源准备失败不留无人持有的地址。
        let mut indexed = Vec::new();
        indexed
            .try_reserve_exact(stack_size / PAGE_SIZE)
            .map_err(|_| NestError::OutOfMemory)?;
        for page in 0..stack_size / PAGE_SIZE {
            indexed.push((
                PageIndex::new(page),
                crate::basis::cell::allocate(self.team.cells.clone())?,
            ));
        }
        let count = PageCount::new(indexed.len()).ok_or(NestError::NotAligned)?;
        let source = Flat::from_cells(self.team.cells.clone(), count, indexed)?.whole();
        let slot = nest.reserve(Placement::highest(
            RegionKind::Normal,
            stack_size + crate::metal::layout::TASK_STACK_GUARD,
        )?)?;
        let (guard, body) = nest.split_reservation(slot, crate::metal::layout::TASK_STACK_GUARD)?;
        stack.guard = Some(guard);
        let stack_body_top = body.start().as_usize() + body.size();
        let rw = Access::READ | Access::WRITE;
        stack.mapping = Some(nest.attach(body, source, rw, rw)?);

        let count = self.args.len();
        let args_at = stack_body_top - count * size_of::<usize>();
        if count > 0 {
            write_args(
                &self.team.nest,
                crate::metal::mmu::internal::canonical(args_at),
                &self.args,
            );
        }

        let start = kernel_api::model::unit::TaskStart {
            entry: self.entry,
            stack_pointer: VirtAddr::wrap(args_at & !0xf),
            arguments: VirtAddr::wrap(args_at),
            argument_count: count,
        };
        let memory = TaskMemory {
            nest: self.team.nest.clone(),
            frame: None,
            bootstrap: Some(stack),
            start,
        };
        super::prepare(self.team, self.controller, start, memory)
    }
}

/// Only boot and Kernel work own a preallocated transition stack. This is never
/// installed by Unit.Create and is not an ownership claim on a Runtime stack.
pub(crate) struct BootstrapStack {
    nest: Arc<Nest>,
    pub(crate) mapping: Option<Arc<crate::basis::nest::Span>>,
    guard: Option<crate::basis::nest::Reservation>,
}
impl BootstrapStack {
    pub(crate) fn retire(&mut self) -> Result<(), NestError> {
        if let Some(mapping) = &self.mapping {
            if !self.nest.settled(mapping.identity()) {
                self.nest.detach(mapping.whole())?;
            }
            self.mapping = None;
        }
        if let Some(guard) = &self.guard {
            if !self.nest.settled(guard.id()) {
                self.nest.release_reserved(guard)?;
            }
            self.guard = None;
        }
        Ok(())
    }
}
impl Drop for BootstrapStack {
    fn drop(&mut self) {
        if let Some(mapping) = self.mapping.take() {
            let _ = self.nest.dispose(mapping);
        }
        if let Some(guard) = self.guard.take() {
            let _ = self.nest.cancel_reservation(guard);
        }
    }
}
