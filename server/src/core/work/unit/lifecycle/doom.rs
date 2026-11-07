use super::reap::reap;
use crate::core::work::room::counter::Counter;
use crate::core::work::room::scheduler::roster::muster;
use crate::core::work::sign;
use crate::core::work::unit::life::TeamLife;
use crate::core::work::unit::task::{Task, TaskExitCause};
use crate::core::work::unit::team::Team;
use alloc::sync::Arc;
use kernel_api::model::identity::TaskId;
static CULL_HELD: Counter = Counter::new();
static CULL_STARVED: Counter = Counter::new();
static CULL_BLOCKED: Counter = Counter::new();
static NUDGED: Counter = Counter::new();
pub(crate) fn branch_stats() -> (usize, usize, usize, usize) {
    (
        CULL_HELD.get(),
        CULL_STARVED.get(),
        CULL_BLOCKED.get(),
        NUDGED.get(),
    )
}
fn suspend(task: &Arc<Task>, cause: TaskExitCause, reason: usize) -> bool {
    let _commit = crate::core::work::commit();
    if task.dead() {
        return false;
    }
    crate::core::work::unit::team::exit::member_dying(task, reason);
    if let Some(hart) = task.on_hart() {
        task.state.lock().doom(Some(hart), cause, reason);
        NUDGED.bump();
        crate::core::work::room::scheduler::idle::nudge(hart);
        return false;
    }
    if task.ready() && !task.stopped() {
        assert!(crate::core::work::room::scheduler::remove_from_starved(
            task
        ));
        CULL_STARVED.bump();
    } else if task.waiting() {
        assert!(sign::detach(task), "published blocked waiter");
        CULL_BLOCKED.bump();
    } else {
        assert!(task.ident.team.release_held(task));
        CULL_HELD.bump();
    }
    if let Some(wait) = task.take_wait() {
        wait.cancel();
    }
    task.state.lock().doom(None, cause, reason);
    true
}
fn cull_life(root: &TeamLife, cause: TaskExitCause, reason: usize) {
    root.doom();
    root.visit(&mut |node| {
        let mut after = 0;
        loop {
            let member = node
                .tasks
                .lock()
                .iter()
                .filter(|m| m.id.get() > after)
                .min_by_key(|m| m.id.get())
                .cloned();
            let Some(member) = member else { break };
            after = member.id.get();
            if let Some(task) = member.task.upgrade() {
                if suspend(&task, cause, reason) {
                    reap(task, cause, reason);
                }
            }
        }
    });
}
pub(crate) fn cull(roots: &[Arc<Team>], reason: usize) {
    for root in roots {
        cull_life(&root.life, TaskExitCause::Slay, reason);
    }
}
pub(crate) fn doom(task: &Arc<Task>) {
    let mut at = 0;
    while let Some(child) = task.heir_node(at) {
        at += 1;
        {
            let _commit = crate::core::work::commit();
            child.life.clear(task.ident.id, None);
        }
        cull_life(&child.life, TaskExitCause::Cascade, super::EXIT_CASCADE);
    }
}
pub(crate) fn take_doomed(tid: TaskId) -> Option<usize> {
    let task = muster(tid)?.upgrade()?;
    task.doomed_on_hart()
}
pub(crate) fn slay(task: &Arc<Task>) {
    if suspend(task, TaskExitCause::Slay, super::EXIT_DOOM) {
        reap(task.clone(), TaskExitCause::Slay, super::EXIT_DOOM);
    }
}
