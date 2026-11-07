use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::{abi::call::unit::UnitFail, model::identity::TaskId, model::identity::TeamId};

use crate::core::memory::nest::NestError;
use crate::core::work::commit;
use crate::core::work::room::scheduler;
use crate::core::work::unit::life::TeamLife;
use crate::core::work::unit::space::Space;
use crate::core::work::unit::task::{Task, TaskBuilder, TaskState, TaskWeak};
use crate::machine::sync::{SpinLock, SpinLockGuard};

mod admission;
mod builder;
mod construct;
pub(crate) mod exit;
mod kernel;
pub(super) mod life;
mod member;
mod oust;
mod phase;
mod receipt;
mod spawn;
mod stage;
mod state;
mod tasks;

pub(crate) use builder::TeamBuilder;
pub(crate) use kernel::{init_kernel, kernel};
pub(crate) use member::Member;
pub(crate) use oust::oust;
pub(crate) use phase::TeamPhase;
pub(crate) use receipt::Receipt;
pub(crate) use spawn::spawn;
pub(crate) use stage::TeamStage;
pub(crate) use state::TeamState;
pub(crate) use tasks::Tasks;

pub(crate) const MAX_DEPTH: usize = 32;
pub(crate) const MAX_MEMBERS: usize = 1024;
pub(crate) const MAX_CHILDREN: usize = 1024;

/// 一个团队：地址空间、身份与家长，加状态机（`TeamState`）与成员表（`Tasks`）。
pub struct Team {
    pub(crate) space: Arc<Space>,
    pub(crate) id: TeamId,
    pub(crate) cells: crate::core::memory::cell::Owner,
    pub(crate) sire: TaskWeak,
    pub(crate) creator: TaskId,
    pub(crate) state: Arc<SpinLock<TeamState>>,
    pub(crate) tasks: Tasks,
    pub(crate) life: Arc<TeamLife>,
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
        self.state.lock().debark()?;
        let mut running = false;
        self.life.visit(&mut |node| {
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
        self.state.lock().embark()?;
        self.life.visit(&mut |node| {
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
                    *task.state.lock() = TaskState::ready();
                    task.ident.team.release_held(&task);
                    scheduler::launch(task);
                }
            }
        });
        Ok(())
    }

    pub(crate) fn paused(&self) -> bool {
        self.life.paused()
    }

    pub(crate) fn ready(&self) -> bool {
        self.state.lock().ready()
    }

    pub(crate) fn guard(&self, owner: TaskId) -> Result<(), UnitFail> {
        let _commit = commit();
        if !self.state.lock().debarked() || self.state.lock().constructing() {
            return Err(UnitFail::Denied);
        }
        if exit::conflicts(&self.life) || self.has_unavailable_members() {
            return Err(UnitFail::Busy);
        }
        exit::arm(&self.life, owner)
    }

    pub(crate) fn unguard(&self, owner: TaskId) -> Result<(), UnitFail> {
        let _commit = commit();
        if !self.state.lock().debarked() || self.state.lock().constructing() {
            return Err(UnitFail::Denied);
        }
        if self.has_unavailable_members() {
            return Err(UnitFail::Busy);
        }
        exit::disarm(&self.life, owner)
    }

    fn has_unavailable_members(&self) -> bool {
        let mut unavailable = false;
        self.life.visit(&mut |node| {
            for member in node.tasks.lock().iter() {
                let state = member.state.lock();
                if state.is_dead() || state.on_hart().is_some() {
                    unavailable = true;
                }
            }
        });
        unavailable
    }

    pub(crate) fn constructing(&self) -> bool {
        self.state.lock().constructing()
    }

    pub(crate) fn operation(&self) -> Option<SpinLockGuard<'_, ()>> {
        self.operation.try_lock()
    }

    pub(crate) fn retire(&self) -> Result<(), NestError> {
        let _operation = self.operation().ok_or(NestError::NoRegion)?;
        {
            let _commit = commit();
            if !self.all_reaped() {
                return Err(NestError::NoRegion);
            }
            // CPU/task cleanup may fail later. DMA retirement must not depend
            // on those steps succeeding or on this Team being dropped.
            self.cells.close();
            self.space.dma.close();
            self.life.visit(&mut |node| {
                let mut state = node.state.lock();
                if !state.terminal() {
                    state.phase = TeamPhase::Doomed;
                }
            });
        }
        let mut after = 0;
        while let Some(task) = scheduler::roster::after(after) {
            after = task.ident.id.get();
            if task.ident.team.id == self.id {
                if !task.reaped() {
                    return Err(NestError::NoRegion);
                }
                task.retire()?;
            }
        }
        self.space.retire()
    }

    pub(crate) fn prune_tasks(&self) {
        let _commit = commit();
        self.life.prune();
    }

    /// Fix the completion cohort: existing members keep their execution states,
    /// while new members and descendant Teams can no longer enter this subtree.
    pub(crate) fn finish(&self) {
        let _commit = commit();
        self.life.visit(&mut |node| {
            let mut state = node.state.lock();
            state.admission = state.admission.finish();
        });
        self.life.notify();
    }

    pub(crate) fn all_reaped(&self) -> bool {
        self.life.all_reaped()
    }

    pub fn task(self: &Arc<Self>) -> TaskBuilder {
        TaskBuilder::new(self.clone())
    }

    pub(crate) fn release_held(&self, task: &Arc<Task>) -> bool {
        self.tasks.release_held(task)
    }

    pub(crate) fn default_entry(&self) -> usize {
        self.state.lock().default_entry()
    }

    pub(crate) fn set_default_entry(&self, va: usize) {
        self.state.lock().set_default_entry(va);
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
        let mut node = Some(&*self.life);
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
        self.state.lock().phase = TeamPhase::Ousted;
        self.life.prune();
        if let Some(parent) = &self.life.parent {
            parent.prune();
        }
    }
}
