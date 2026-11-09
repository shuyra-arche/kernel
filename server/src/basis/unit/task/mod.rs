use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;
use kernel_api::model::cell::MappingControl;

use kernel_api::abi::call::unit::UnitFail;
use kernel_api::model::identity::TeamId;

use crate::basis::commit;
use crate::basis::pend::Table;
use crate::basis::room::scheduler;
use crate::basis::sign::Ticket;
use crate::basis::sign::WakeKey;
use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::lifecycle::tally;
use crate::basis::unit::team::Team;
use crate::basis::unit::team::TeamPhase;
use crate::metal::hart::HartId;
use crate::metal::sync::SpinLock;

use crate::basis::unit::control::Request;
use execution::Effect;

pub(crate) mod chain;
mod creation;
mod execution;
mod identity;
pub(crate) mod registry;
mod weak;

pub(crate) use creation::TaskBuilder;
pub(crate) use execution::{Resumption, TaskState, WaitCall};
pub(crate) use identity::TaskIdent;
#[cfg(debug_assertions)]
pub(crate) use weak::check_block_heldout;
pub(crate) use weak::{Origin, TaskWeak};

pub use kernel_api::model::unit::ExitCause as TaskExitCause;

pub(crate) const MAX_ARGS: usize = 64;

/// 一个任务：身份（`TaskIdent`，含它自己的内存段）、状态机（`TaskState`）与能力
/// 账本（`Pend`）的组合，加上它作为家长持有的子团队表。
pub struct Task {
    #[cfg(feature = "latency")]
    pub(crate) ready_stamp: core::sync::atomic::AtomicU64,
    pub(crate) ident: Arc<TaskIdent>,
    pub(crate) life: Arc<Anchor>,
    pub(crate) state: Arc<SpinLock<TaskState>>,
    pub(crate) pend: Table,
    pub(crate) administration: crate::basis::unit::control::Table,
    pub(crate) heir: SpinLock<Vec<Arc<Team>>>,
    extensions: SpinLock<crate::metal::switcher::context::extension::ExtendedContext>,
    controls: SpinLock<Vec<(Weak<crate::basis::nest::Nest>, usize)>>,
}

// Kernel Tasks may migrate between harts. Mutable state is behind SpinLock or
// atomics; trap-frame access is serialized by execution ownership and COMMIT.
// API PendId's !Send marker in the ledger describes user-thread ownership, not
// kernel storage. No guard or unsynchronized mutable reference leaves a Task.
unsafe impl Send for Task {}
unsafe impl Sync for Task {}

impl Task {
    pub(crate) fn execution_capability(
        hart: HartId,
    ) -> crate::metal::switcher::context::extension::Capability {
        crate::metal::switcher::context::extension::hardware::capability(hart)
    }

    pub(crate) fn can_run_on(&self, hart: HartId) -> bool {
        self.extensions
            .lock()
            .compatible(Self::execution_capability(hart))
    }

    /// Called before publishing any waiting/ready/terminal transition.
    pub(crate) unsafe fn capture_extensions(&self, status: usize) {
        assert_eq!(
            self.on_hart(),
            Some(crate::metal::hart::hart_id()),
            "capture without execution ownership"
        );
        unsafe { self.extensions.lock().capture(status) };
    }

    pub(crate) unsafe fn restore_extensions(&self, status: &mut usize) {
        assert_eq!(
            self.on_hart(),
            Some(crate::metal::hart::hart_id()),
            "restore without execution ownership"
        );
        assert!(
            self.can_run_on(crate::metal::hart::hart_id()),
            "incompatible execution Hart"
        );
        unsafe { self.extensions.lock().restore(status) };
    }

    /// Reaping runs only after the running/waiting/ready owner has detached.
    /// Extended CPU images have no asynchronous hardware obligations; release
    /// them now, even if other resources retain a cleanup husk or result record.
    pub(crate) fn release_extensions(&self) {
        let image = {
            let _commit = commit();
            assert!(
                self.dead() && self.on_hart().is_none(),
                "release before execution stops"
            );
            core::mem::take(&mut *self.extensions.lock())
        };
        drop(image);
    }

    pub(crate) fn enable_extension(
        &self,
        instruction: u32,
        status: &mut usize,
    ) -> Result<bool, ()> {
        self.extensions.lock().enable(
            instruction,
            status,
            Self::execution_capability(crate::metal::hart::hart_id()),
        )
    }

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
        task.state.lock().transform(TaskState::ready());
        scheduler::insert(task.clone());
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
                if !scheduler::remove(task) {
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
                scheduler::insert(task.clone());
                Ok(())
            }
            other => unreachable!("resume effect: {other:?}"),
        }
    }

    pub(crate) fn park(task: Arc<Task>) {
        #[cfg(feature = "latency")]
        task.ready_stamp
            .store(0, core::sync::atomic::Ordering::Relaxed);
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
        child.node.state.lock().phase = TeamPhase::Ousted;
        child.node.clear(self.ident.id, Some(team));
        let mut root = child.node.clone();
        while let Some(parent) = &root.parent {
            root = parent.clone();
        }
        root.prune();
        Some(child)
    }

    pub(crate) fn track_control(&self, space: &Arc<Nest>, id: usize) -> Result<(), NestError> {
        let mut controls = self.controls.lock();
        controls
            .try_reserve(1)
            .map_err(|_| NestError::OutOfMemory)?;
        controls.push((Arc::downgrade(space), id));
        Ok(())
    }

    /// 所有 Nest 共用单调、不复用的关系号；id 唯一标识这一笔控制。
    pub(crate) fn release_control(&self, id: usize) {
        self.controls.lock().retain(|(_, held)| *held != id);
    }

    /// 任务退场时收掉它控制的每一段关系；未结的登记原地留在本表里等既有重试。
    ///
    /// 返回 `Err` 时表里仍有未结的控制：调用方（`Task::retire` / HUSKS）必须保留
    /// 这个 Task 并再次调用，不能把仍有作用的控制当成已经结清。
    /// `Nest` 已消失的那一档是 raw last-drop 的既有边界：没有 Nest 可重试，也不能
    /// 声称结清；它与“已证明结清”分开，只撤掉无法再走的控制记录。
    pub(crate) fn close_controls(&self) -> Result<(), NestError> {
        let mut at = 0;
        let mut failure: Option<NestError> = None;
        loop {
            let Some((space, id)) = self
                .controls
                .lock()
                .get(at)
                .map(|(space, id)| (space.clone(), *id))
            else {
                break;
            };
            let Some(space) = space.upgrade() else {
                self.release_control(id);
                continue;
            };
            // 一把 registry 锁取任意登记：中间提交的槽不会在两次查询里丢掉。
            let Some(entry) = space.mappings.registered(id) else {
                self.release_control(id);
                continue;
            };
            let Some(control) = entry.control.as_ref() else {
                self.release_control(id);
                continue;
            };
            let (controller, _) = control.control();
            if controller != self.ident.id || control.scope() != MappingControl::Caller {
                self.release_control(id);
                continue;
            }
            let retry = if space.settled(id) {
                Ok(())
            } else {
                if let Some(mapping) = entry.mapping() {
                    let _ = space.dispose(mapping.clone());
                }
                space.retry_invalidation()
            };
            if !space.settled(id) {
                failure = failure.or(retry.err()).or(Some(NestError::Busy));
                at += 1;
            } else {
                // 结清只有一条规则：Registry 项与源侧成员一起撤，controller 也跟着放。
                space.retire_mapping(id);
            }
            // 沿原行为释放这个空间里其他已结清的 controller（不在本表锁下）。
            space.reap_mappings();
        }
        // 并发删除可能让游标跳过一项；表非空就不能当成已全部结清。
        if self.controls.lock().is_empty() {
            Ok(())
        } else {
            Err(failure.unwrap_or(NestError::Busy))
        }
    }

    pub(crate) fn retire(self: &Arc<Self>) -> Result<(), crate::basis::nest::NestError> {
        let mut at = 0;
        while let Some(child) = self.heir_node(at) {
            child.retire()?;
            at += 1;
        }
        crate::basis::pend::retire(self).map_err(|_| crate::basis::nest::NestError::Shootdown)?;
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
            root.node.visit(&mut |node| {
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
        found.filter(|team| !team.node.state.lock().phase.ousted())
    }

    pub(crate) fn life(&self) -> Weak<Anchor> {
        Arc::downgrade(&self.life)
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        let _commit = commit();
        self.administration.clear();
        self.ident.team.node.clear(self.ident.id, None);
        self.ident.team.node.prune();
        for child in self.heir.lock().iter() {
            child.node.clear(self.ident.id, None);
            child.node.prune();
        }
        for pend_entry in self.pend.entries.lock().iter() {
            pend_entry.invalidate();
        }
        self.count_vanished();
    }
}
