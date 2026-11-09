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
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskBuilder;
use crate::basis::unit::task::TaskWeak;
use crate::metal::sync::SpinLock;
use crate::metal::sync::SpinLockGuard;

mod builder;
pub(crate) mod capsule;
pub(super) mod lifecycle;
mod membership;

pub(crate) use builder::{TeamBuilder, init_kernel, kernel};
pub(crate) use lifecycle::exit;
pub(crate) use lifecycle::{TeamNode, TeamPhase};
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
    operation: SpinLock<()>,
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

    pub(crate) fn constructing(&self) -> bool {
        self.node.state.lock().constructing()
    }

    pub(crate) fn operation(&self) -> Option<SpinLockGuard<'_, ()>> {
        self.operation.try_lock()
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
            let mut state = self.node.state.lock();
            if !state.terminal() {
                state.phase = TeamPhase::Doomed;
            }
            drop(state);
            if members
                .iter()
                .any(|member| !member.reclaimed.load(core::sync::atomic::Ordering::Acquire))
            {
                return Err(NestError::Busy);
            }
        }
        self.nest.retire()
    }

    pub(crate) fn prune_tasks(&self) {
        let _commit = commit();
        self.node.prune();
    }

    /// Fix the completion cohort: existing members keep their execution states,
    /// while new members and descendant Teams can no longer enter this subtree.
    pub(crate) fn finish(&self) {
        let _commit = commit();
        self.node.visit(&mut |node| {
            let mut state = node.state.lock();
            state.admission = state.admission.finish();
        });
        self.node.notify();
    }

    pub(crate) fn all_reaped(&self) -> bool {
        self.node.all_reaped()
    }

    pub fn task(self: &Arc<Self>) -> TaskBuilder {
        TaskBuilder::new(self.clone())
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
        self.node.state.lock().phase = TeamPhase::Ousted;
        self.node.prune();
        if let Some(parent) = &self.node.parent {
            parent.prune();
        }
    }
}

/// Publish execution after entry/stack preparation. Data access is delegated separately through Pend.
pub(crate) fn spawn(
    target: &Arc<Team>,
    caller: Option<&Arc<Task>>,
    entry: usize,
    words: Vec<usize>,
    stack: usize,
) -> Result<Arc<Task>, UnitFail> {
    fn map_err(error: NestError) -> UnitFail {
        if error == NestError::OutOfMemory {
            UnitFail::OutOfMemory
        } else {
            UnitFail::Denied
        }
    }
    let _construction = target.operation().ok_or(UnitFail::Busy)?;
    let first = !target.ready();
    let entry_va = if entry == 0 {
        target.default_entry()
    } else {
        entry
    };
    // 入口的前两个字节都要取得到：取不到就拒收。
    let entry = crate::metal::mmu::canonical(entry_va);
    if !entry_va.is_multiple_of(2)
        || target.nest.instruction_byte(entry).is_none()
        || target.nest.instruction_byte(entry + 1).is_none()
    {
        return Err(UnitFail::BadEntry);
    }
    let mut builder = target
        .task()
        .entry(crate::metal::mmu::canonical(entry_va))
        .args(words);
    if let Some(caller) = caller {
        builder = builder.controller(caller.ident.id);
    }
    if stack > 0 {
        builder = builder.stack(stack);
    }
    let mut prepared = builder.prepare().map_err(map_err)?;
    prepared
        .publish(|| {
            if first {
                target.set_default_entry(entry_va);
            }
        })
        .map_err(map_err)
}

pub(crate) fn oust(me: &Arc<Task>, id: TeamId) -> Result<(), UnitFail> {
    let child = me.clan(id).ok_or(UnitFail::Denied)?;
    child.retire().map_err(|_| UnitFail::Busy)?;
    let operation = child.operation().ok_or(UnitFail::Busy)?;
    {
        let _commit = commit();
        if !child.all_reaped() {
            return Err(UnitFail::Busy);
        }
    }
    let retired = {
        let _commit = commit();
        child.node.state.lock().phase = TeamPhase::Ousted;
        child.node.clear(me.ident.id, None);
        let sire = child.sire.upgrade();
        let detached = sire.as_ref().and_then(|sire| sire.oust(id));
        child.node.prune();
        (sire, detached)
    };
    drop(operation);
    drop(retired);
    Ok(())
}
