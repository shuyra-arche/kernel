use super::reap::reap;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::basis::sign;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskExitCause;
use crate::basis::unit::task::registry::find;
use crate::basis::unit::team::Team;
use crate::basis::unit::team::TeamNode;
use alloc::sync::Arc;
use kernel_api::model::identity::TaskId;
static CULL_HELD: AtomicUsize = AtomicUsize::new(0);
static CULL_READY: AtomicUsize = AtomicUsize::new(0);
static CULL_BLOCKED: AtomicUsize = AtomicUsize::new(0);
static NUDGED: AtomicUsize = AtomicUsize::new(0);
pub(crate) fn branch_stats() -> (usize, usize, usize, usize) {
    (
        CULL_HELD.load(Ordering::Relaxed),
        CULL_READY.load(Ordering::Relaxed),
        CULL_BLOCKED.load(Ordering::Relaxed),
        NUDGED.load(Ordering::Relaxed),
    )
}
fn suspend(task: &Arc<Task>, cause: TaskExitCause, reason: usize) -> bool {
    let _commit = crate::basis::commit();
    if task.dead() {
        return false;
    }
    crate::basis::unit::team::exit::member_dying(task, reason);
    if let Some(hart) = task.on_hart() {
        task.state.lock().doom(Some(hart), cause, reason);
        NUDGED.fetch_add(1, Ordering::Relaxed);
        crate::basis::room::scheduler::idle::nudge(hart);
        return false;
    }
    if task.ready() && !task.stopped() {
        assert!(crate::basis::room::scheduler::remove(task));
        CULL_READY.fetch_add(1, Ordering::Relaxed);
    } else if task.waiting() {
        assert!(sign::detach(task), "published blocked waiter");
        CULL_BLOCKED.fetch_add(1, Ordering::Relaxed);
    } else {
        assert!(task.ident.team.release_held(task));
        CULL_HELD.fetch_add(1, Ordering::Relaxed);
    }
    if let Some(wait) = task.take_wait() {
        wait.cancel();
    }
    task.state.lock().doom(None, cause, reason);
    true
}
fn cull_life(root: &TeamNode, cause: TaskExitCause, reason: usize) {
    root.doom();
    root.visit(&mut |node| {
        let mut after = 0;
        loop {
            let member = {
                let tasks = node.tasks.lock();
                tasks
                    .get(tasks.partition_point(|m| m.id.get() <= after))
                    .cloned()
            };
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
        cull_life(&root.node, TaskExitCause::Slay, reason);
    }
}
pub(crate) fn doom(task: &Arc<Task>) {
    let mut at = 0;
    while let Some(child) = task.heir_node(at) {
        at += 1;
        {
            let _commit = crate::basis::commit();
            child.node.clear(task.ident.id, None);
        }
        cull_life(&child.node, TaskExitCause::Cascade, super::EXIT_CASCADE);
    }
}
pub(crate) fn take_doomed(tid: TaskId) -> Option<usize> {
    let task = find(tid)?.upgrade()?;
    task.doomed_on_hart()
}
pub(crate) fn slay(task: &Arc<Task>) {
    if suspend(task, TaskExitCause::Slay, super::EXIT_DOOM) {
        reap(task.clone(), TaskExitCause::Slay, super::EXIT_DOOM);
    }
}
