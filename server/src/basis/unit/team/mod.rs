use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::abi::call::unit::UnitFail;
use kernel_api::model::identity::TaskId;
use kernel_api::model::identity::TeamId;

use crate::basis::commit;
use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::authority::admission::Admission;
use crate::basis::room::scheduler;
use crate::basis::unit::task::Bootstrap;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskWeak;

mod builder;
pub(crate) mod capsule;
pub(super) mod lifecycle;
mod membership;

pub(crate) use builder::{TeamBuilder, init_kernel, kernel};
pub(crate) use lifecycle::exit;
pub(crate) use lifecycle::{Clearance, TeamExit, TeamNode};
pub(crate) use membership::{Member, Receipt, Tasks};

pub(crate) const MAX_DEPTH: usize = 32;
pub(crate) const MAX_MEMBERS: usize = 1024;
pub(crate) const MAX_CHILDREN: usize = 1024;

/// 一个团队：地址空间、身份与家长，加状态机（`TeamState`）与成员表（`Tasks`）。
pub struct Team {
    pub(crate) nest: Arc<Nest>,
    pub(crate) external: Admission,
    pub(crate) id: TeamId,
    pub(crate) cells: crate::basis::cell::page::Owner,
    pub(crate) sire: TaskWeak,
    pub(crate) creator: TaskId,
    pub(crate) tasks: Tasks,
    pub(crate) node: Arc<TeamNode>,
    operation: core::sync::atomic::AtomicBool,
}

unsafe impl Sync for Team {}
unsafe impl Send for Team {}

impl Team {
    pub(crate) fn park(&self, task: Arc<Task>) {
        self.tasks.park(task);
    }

    pub(crate) fn debark(&self) -> Result<(), UnitFail> {
        let _commit = commit();
        self.node.state.lock().debark()?;
        let mut running = false;
        self.node.visit(&mut |node| {
            for member in node.tasks.lock().iter() {
                let hart = member.state.lock().on_hart();
                if let Some(hart) = hart {
                    running = true;
                    scheduler::idle::nudge(hart);
                }
            }
        });
        if running {
            return Err(UnitFail::Busy);
        }
        Ok(())
    }

    pub(crate) fn embark(&self) -> Result<(), UnitFail> {
        let _commit = commit();
        self.node.state.lock().embark()?;
        self.node.visit(&mut |node| {
            if node.paused() {
                return;
            }
            let mut at = 0;
            loop {
                let member = node.tasks.lock().get(at).cloned();
                let Some(member) = member else { break };
                at += 1;
                let Some(task) = member.task.upgrade() else {
                    continue;
                };
                if task.parked() {
                    task.state.lock().rise();
                    task.ident.team.release_held(&task);
                    scheduler::insert(task);
                }
            }
        });
        Ok(())
    }

    pub(crate) fn paused(&self) -> bool {
        self.node.paused()
    }

    pub(crate) fn ready(&self) -> bool {
        self.node.state.lock().ready()
    }

    pub(crate) fn operation(&self) -> Option<Operation<'_>> {
        self.operation
            .compare_exchange(
                false,
                true,
                core::sync::atomic::Ordering::Acquire,
                core::sync::atomic::Ordering::Relaxed,
            )
            .ok()?;
        Some(Operation(&self.operation))
    }

    pub(crate) fn retire(&self) -> Result<(), NestError> {
        let _operation = self.operation().ok_or(NestError::Busy)?;
        {
            let _commit = commit();
            let members = self.node.tasks.lock();
            if members.iter().any(|member| member.exit().is_none()) {
                return Err(NestError::Busy);
            }
            // Each member's husk owns its retirement. Never recursively retire
            // another Task here or scan the system-wide Task registry. A removed
            // member has already published reclaimed before pruning.
            self.cells.close();
            self.external.close();
            self.node.state.lock().doom(None);
            if members
                .iter()
                .any(|member| !member.reclaimed.load(core::sync::atomic::Ordering::Acquire))
            {
                return Err(NestError::Busy);
            }
        }
        self.nest.retire()
    }

    /// Fix the completion cohort: existing members keep their execution states,
    /// while new members and descendant Teams can no longer enter this subtree.
    pub(crate) fn finish(&self) {
        let _commit = commit();
        // Every descendant admission checks its ancestors. One root transition
        // fixes the whole cohort without walking it under COMMIT.
        self.node.state.lock().finish();
        self.node.notify();
    }

    pub(crate) fn all_reaped(&self) -> bool {
        self.node.all_reaped()
    }

    pub fn task(self: &Arc<Self>) -> Bootstrap {
        Bootstrap::new(self.clone())
    }

    pub(crate) fn release_held(&self, task: &Arc<Task>) -> bool {
        self.tasks.release_held(task)
    }

    pub(crate) fn default_entry(&self) -> usize {
        self.node.state.lock().default_entry()
    }

    pub(crate) fn set_default_entry(&self, va: usize) {
        self.node.state.lock().set_default_entry(va);
    }

    pub(crate) fn sire(&self) -> Option<TaskId> {
        self.sire.upgrade().map(|t| t.ident.id)
    }

    /// 实际调用 `Unit::Build` 的那一台；内核直建/启动团队为 0。
    pub(crate) fn creator(&self) -> TaskId {
        self.creator
    }

    /// 新任务的收据：从这棵树往上数，每个有主（`owner != 0`）的节点都算一条，
    /// 按家长号合并；深度超限或撞上已关的节点即拒绝。
    pub(crate) fn receipts(&self) -> Result<Vec<Receipt>, NestError> {
        let mut receipts: Vec<Receipt> = Vec::new();
        let mut node = Some(&*self.node);
        let mut depth = 0;
        while let Some(here) = node {
            depth += 1;
            if depth > MAX_DEPTH || here.closed() {
                return Err(NestError::NoRegion);
            }
            if here.owner.get() != 0 {
                if let Some(r) = receipts.iter_mut().find(|r| r.owner == here.owner) {
                    r.roots.try_reserve(1).map_err(|_| NestError::OutOfMemory)?;
                    r.roots.push(here.id);
                } else {
                    let mut roots = Vec::new();
                    roots.try_reserve(1).map_err(|_| NestError::OutOfMemory)?;
                    roots.push(here.id);
                    receipts
                        .try_reserve(1)
                        .map_err(|_| NestError::OutOfMemory)?;
                    receipts.push(Receipt {
                        owner: here.owner,
                        roots,
                    });
                }
            }
            node = here.parent.as_deref();
        }
        Ok(receipts)
    }
}

impl Drop for Team {
    fn drop(&mut self) {
        self.cells.close();
        let _commit = commit();
        self.node.state.lock().discard();
        self.node.prune();
    }
}

/// Publish an ordinary User execution context. The caller prepared all user
/// memory through its existing, separately authorized Nest/Flat operations.
pub(crate) fn spawn(
    target: &Arc<Team>,
    caller: &Arc<Task>,
    start: kernel_api::model::unit::TaskStart,
) -> Result<Arc<Task>, UnitFail> {
    fn map_err(error: NestError) -> UnitFail {
        match error {
            NestError::OutOfMemory => UnitFail::OutOfMemory,
            NestError::Busy => UnitFail::Busy,
            _ => UnitFail::Denied,
        }
    }
    let _construction = target.operation().ok_or(UnitFail::Busy)?;
    let validate = || {
        if !crate::basis::pend::live(caller)
            || target.nest.kind().is_supervisor()
            || !(Arc::ptr_eq(target, &caller.ident.team)
                || caller
                    .clan(target.id)
                    .is_some_and(|team| Arc::ptr_eq(&team, target)))
        {
            return Err(NestError::WidenDenied);
        }
        target.nest.validate_start(start)
    };
    validate().map_err(map_err)?;
    let first = !target.ready();
    let mut prepared =
        crate::basis::unit::task::prepare_user(target.clone(), caller.ident.id, start)
            .map_err(map_err)?;
    // COMMIT excludes revocation while the final authority/mapping checks and
    // execution publication commit. The creation operation excludes Team retire.
    prepared
        .publish(validate, || {
            if first {
                target.set_default_entry(start.entry.get());
            }
        })
        .map_err(map_err)
}

pub(crate) fn oust(me: &Arc<Task>, id: TeamId) -> Result<(), UnitFail> {
    let child = me.clan(id).ok_or(UnitFail::Denied)?;
    child.retire().map_err(|_| UnitFail::Busy)?;
    let _retired = {
        let _operation = child.operation().ok_or(UnitFail::Busy)?;
        let _commit = commit();
        if !child.all_reaped() {
            return Err(UnitFail::Busy);
        }
        child.node.state.lock().oust()?;
        child.node.clear(me.ident.id, None);
        let sire = child.sire.upgrade();
        let detached = sire.as_ref().and_then(|sire| sire.detach_heir(id));
        (sire, detached)
    };
    Ok(())
}

// A nonblocking operation lease serializes construction/retirement without
// retaining a software lock or masking IRQs over publication or completion.
pub(crate) struct Operation<'a>(&'a core::sync::atomic::AtomicBool);
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.0.store(false, core::sync::atomic::Ordering::Release);
    }
}
