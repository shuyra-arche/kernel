use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use kernel_api::{abi::call::unit::UnitFail, model::identity::TeamId};

use crate::basis::work::commit;
use crate::basis::work::pend::Table;
use crate::basis::work::room::scheduler;
use crate::basis::work::sign::{Ticket, WakeKey};
use crate::basis::work::unit::life::Life;
use crate::basis::work::unit::tally;
use crate::basis::work::unit::team::{Team, TeamPhase};
use crate::machine::hart::HartId;
use crate::machine::sync::SpinLock;

use crate::basis::work::unit::control::Request;
use state::control::Effect;

mod builder;
mod continuation;
mod control;
pub(crate) use continuation::{Resumption, WaitCall};
mod ident;
pub(super) mod life;
mod prepared;
mod state;
mod weak;

pub(crate) use builder::TaskBuilder;
pub(crate) use ident::TaskIdent;
pub(crate) use state::TaskState;
pub(crate) use weak::TaskWeak;

pub use kernel_api::model::unit::ExitCause as TaskExitCause;

pub(crate) const MAX_ARGS: usize = 64;

/// 一个任务：身份（`TaskIdent`，含它自己的内存段）、状态机（`TaskState`）与能力
/// 账本（`Pend`）的组合，加上它作为家长持有的子团队表。
pub struct Task {
    pub(crate) ident: Arc<TaskIdent>,
    pub(crate) life: Arc<Life>,
    pub(crate) state: Arc<SpinLock<TaskState>>,
    pub(crate) pend: Table,
    pub(crate) signs: crate::basis::work::sign::Table,
    pub(crate) administration: crate::basis::work::unit::control::Table,
    pub(crate) heir: SpinLock<Vec<Arc<Team>>>,
    controls: SpinLock<Vec<(Weak<crate::basis::work::unit::space::Space>, usize)>>,
}

unsafe impl Send for Task {}

impl Task {
    pub(crate) fn transform_checked(&self, next: TaskState) {
        self.state.lock().transform(next);
    }

    pub fn held(&self) -> bool {
        self.state.lock().is_held()
    }

    pub(crate) fn parked(&self) -> bool {
        self.state.lock().is_parked()
    }

    pub fn dead(&self) -> bool {
        self.state.lock().is_dead()
    }

    pub(crate) fn doomed(&self) -> bool {
        self.state.lock().is_doomed()
    }

    pub(crate) fn reaped(&self) -> bool {
        self.state.lock().is_reaped()
    }

    pub(crate) fn stopped(&self) -> bool {
        self.state.lock().paused()
    }

    pub(crate) fn on_hart(&self) -> Option<HartId> {
        self.state.lock().on_hart()
    }

    pub(crate) fn ticks_left(&self) -> u32 {
        self.state.lock().ticks_left()
    }

    pub(crate) fn doomed_on_hart(&self) -> Option<usize> {
        self.state.lock().doomed_on_hart()
    }

    pub(crate) fn block(&self, key: WakeKey, ticket: Ticket, wait: Option<WaitCall>) {
        self.state.lock().block(key, ticket, wait);
    }

    pub(crate) fn ready(&self) -> bool {
        self.state.lock().is_ready()
    }

    pub(crate) fn waiting(&self) -> bool {
        self.state.lock().is_waiting()
    }

    pub(crate) fn next(&self) -> Option<Arc<Task>> {
        self.state.lock().link().clone()
    }

    pub(crate) fn set_next(&self, value: Option<Arc<Task>>) {
        *self.state.lock().link() = value;
    }

    pub(crate) fn take_next(&self) -> Option<Arc<Task>> {
        self.state.lock().link().take()
    }

    pub(crate) fn wait_ticket(&self) -> Ticket {
        self.state.lock().wait_ticket()
    }

    pub(crate) fn take_wait(&self) -> Option<WaitCall> {
        self.state.lock().take_wait()
    }

    pub(crate) fn dec_ticks_left(&self) {
        self.state.lock().dec_ticks_left();
    }

    pub(crate) fn release(task: &Arc<Task>) -> Result<(), UnitFail> {
        let _commit = commit();
        if !task.held() || !task.ident.team.release_held(task) {
            return Err(UnitFail::Denied);
        }
        *task.state.lock() = TaskState::ready();
        scheduler::launch(task.clone());
        Ok(())
    }

    pub(crate) fn debark(task: &Arc<Task>) -> Result<(), UnitFail> {
        let _commit = commit();
        let transition = task.state.lock().control().transition(Request::Pause)?;
        match transition.effect {
            Effect::None => {
                task.state.lock().apply_control(transition);
                Ok(())
            }
            Effect::Dequeue => {
                // Do not publish the paused state if queue removal fails.
                if !scheduler::remove_from_starved(task) {
                    return Err(UnitFail::Busy);
                }
                task.state.lock().apply_control(transition);
                task.ident.team.park(task.clone());
                Ok(())
            }
            Effect::Interrupt => {
                let hart = task.on_hart().expect("running control target");
                task.state.lock().apply_control(transition);
                scheduler::idle::nudge(hart);
                Err(UnitFail::Busy)
            }
            other => unreachable!("pause effect: {other:?}"),
        }
    }

    pub(crate) fn embark(task: &Arc<Task>) -> Result<(), UnitFail> {
        let _commit = commit();
        if task.held() {
            return Self::release(task);
        }
        let transition = task.state.lock().control().transition(Request::Resume)?;
        task.state.lock().apply_control(transition);
        match transition.effect {
            Effect::None => Ok(()),
            Effect::Release => Self::release(task),
            Effect::Enqueue => {
                task.ident.team.release_held(task);
                scheduler::launch(task.clone());
                Ok(())
            }
            other => unreachable!("resume effect: {other:?}"),
        }
    }

    pub(crate) fn park(task: Arc<Task>) {
        let team = task.ident.team.clone();
        task.state.lock().park();
        team.park(task);
    }

    pub(crate) fn rise(task: &Arc<Task>) -> bool {
        let paused = task.state.lock().rise();
        if paused {
            task.ident.team.park(task.clone());
            false
        } else {
            true
        }
    }

    fn count_vanished(&self) {
        if !self.reaped() {
            tally::exit();
        }
    }

    pub(crate) fn oust(&self, team: TeamId) -> Option<Arc<Team>> {
        let _commit = commit();
        let child = {
            let mut heirs = self.heir.lock();
            let at = heirs.iter().position(|t| t.id == team)?;
            heirs.remove(at)
        };
        child.state.lock().phase = TeamPhase::Ousted;
        child.life.clear(self.ident.id, Some(team));
        let mut root = child.life.clone();
        while let Some(parent) = &root.parent {
            root = parent.clone();
        }
        root.prune();
        Some(child)
    }

    pub(crate) fn retire(self: &Arc<Self>) -> Result<(), crate::basis::memory::nest::NestError> {
        let mut at = 0;
        while let Some(child) = self.heir_node(at) {
            child.retire()?;
            at += 1;
        }
        crate::basis::work::pend::retire(self)
            .map_err(|_| crate::basis::memory::nest::NestError::Shootdown)?;
        // 子空间先完成退休，才可解除本任务留在其中的控制；未结时仍保留任务重试。
        self.close_controls()?;
        self.ident.retire()
    }

    pub(crate) fn heir_node(&self, at: usize) -> Option<Arc<Team>> {
        self.heir.lock().get(at).cloned()
    }

    pub(crate) fn heir(&self, id: TeamId) -> Option<Arc<Team>> {
        self.heir.lock().iter().find(|t| t.id == id).cloned()
    }

    pub(crate) fn clan(&self, id: TeamId) -> Option<Arc<Team>> {
        let _commit = commit();
        if id == self.ident.team.id {
            return None;
        }
        if let Some(child) = self.heir(id) {
            return Some(child);
        }
        let mut found = None;
        // Execution authority follows teams created by this Task, not membership
        // in the Task's own Team. Peers do not inherit each other's children.
        let mut index = 0;
        while let Some(root) = self.heir_node(index) {
            index += 1;
            root.life.visit(&mut |node| {
                if node.id != id {
                    return;
                }
                found = node
                    .parent
                    .as_ref()
                    .and_then(|parent| parent.member(node.owner))
                    .and_then(|member| member.task.upgrade())
                    .and_then(|sire| sire.heir(id))
                    .or_else(|| {
                        let mut at = 0;
                        loop {
                            let member = node.tasks.lock().get(at).cloned()?;
                            at += 1;
                            if let Some(task) = member.task.upgrade() {
                                return Some(task.ident.team.clone());
                            }
                        }
                    });
            });
        }
        found.filter(|team| !team.state.lock().phase.ousted())
    }

    pub(crate) fn life(&self) -> Weak<Life> {
        Arc::downgrade(&self.life)
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        let _commit = commit();
        self.signs.clear(self.ident.id);
        self.administration.clear();
        self.ident.team.life.clear(self.ident.id, None);
        self.ident.team.life.prune();
        for child in self.heir.lock().iter() {
            child.life.clear(self.ident.id, None);
            child.life.prune();
        }
        for pend_entry in self.pend.entries.lock().iter() {
            pend_entry.invalidate();
        }
        self.count_vanished();
    }
}
