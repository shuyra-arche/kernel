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
/// A Team owns its deferred sweep, including queue membership and both cursors.
/// Logical closure is the Team phase; the sweep never grants authority.
pub(crate) enum Termination {
    Idle,
    Queued {
        work: Sweep,
        next: Option<Arc<TeamNode>>,
    },
    Running,
    Settled,
}
impl Termination {
    pub(crate) const fn new() -> Self {
        Self::Idle
    }
}
pub(crate) struct Sweep {
    cause: TaskExitCause,
    reason: usize,
    member: usize,
    child: usize,
}
struct Queue {
    first: Option<Arc<TeamNode>>,
    last: Option<Arc<TeamNode>>,
}
static OUTSTANDING: AtomicUsize = AtomicUsize::new(0);
pub(super) fn settled() -> bool {
    OUTSTANDING.load(Ordering::Acquire) == 0
}
static PENDING: crate::metal::sync::SpinLock<Queue> = crate::metal::sync::SpinLock::new(Queue {
    first: None,
    last: None,
});
impl Queue {
    fn push(&mut self, node: Arc<TeamNode>, work: Sweep) {
        *node.termination.lock() = Termination::Queued { work, next: None };
        if let Some(last) = self.last.take() {
            let mut state = last.termination.lock();
            let Termination::Queued { next, .. } = &mut *state else {
                panic!("unlinked termination tail")
            };
            *next = Some(node.clone());
        } else {
            self.first = Some(node.clone());
        }
        self.last = Some(node);
    }
    fn pop(&mut self) -> Option<(Arc<TeamNode>, Sweep)> {
        let node = self.first.take()?;
        let Termination::Queued { work, next } =
            core::mem::replace(&mut *node.termination.lock(), Termination::Running)
        else {
            panic!("unlinked termination head")
        };
        self.first = next;
        if self.first.is_none() {
            self.last = None;
        }
        Some((node, work))
    }
}
fn request(node: &Arc<TeamNode>, cause: TaskExitCause, reason: usize) {
    let _commit = crate::basis::commit();
    let mut pending = PENDING.lock();
    if !matches!(*node.termination.lock(), Termination::Idle) {
        return;
    }
    {
        let mut state = node.state.lock();
        if !state.terminal() {
            state.phase = crate::basis::unit::team::TeamPhase::Doomed;
        }
    }
    OUTSTANDING.fetch_add(1, Ordering::Release);
    pending.push(
        node.clone(),
        Sweep {
            cause,
            reason,
            member: 0,
            child: 0,
        },
    );
    crate::basis::room::deadline::retry();
}
/// Four local member/child steps. The queue itself supplies storage, so failure
/// to allocate cannot discard an accepted termination request.
pub(super) fn advance() {
    for _ in 0..4 {
        let Some((node, mut work)) = PENDING.lock().pop() else {
            break;
        };
        let member = {
            let members = node.tasks.lock();
            members
                .get(members.partition_point(|m| m.id.get() <= work.member))
                .cloned()
        };
        let more = if let Some(member) = member {
            work.member = member.id.get();
            if let Some(task) = member.task.upgrade() {
                if suspend(&task, work.cause, work.reason) {
                    reap(task, work.cause, work.reason);
                }
            }
            true
        } else {
            let child = {
                let children = node.children.lock();
                children
                    .get(children.partition_point(|c| c.id.get() <= work.child))
                    .cloned()
            };
            if let Some(child) = child {
                work.child = child.id.get();
                request(&child, work.cause, work.reason);
                true
            } else {
                false
            }
        };
        if more {
            PENDING.lock().push(node, work);
        } else {
            *node.termination.lock() = Termination::Settled;
            OUTSTANDING.fetch_sub(1, Ordering::Release);
        }
    }
    if PENDING.lock().first.is_some() {
        crate::basis::room::deadline::retry();
    }
}
pub(crate) fn cull(roots: &[Arc<Team>], reason: usize) {
    for root in roots {
        request(&root.node, TaskExitCause::Slay, reason);
    }
}
pub(crate) fn doom(task: &Arc<Task>) {
    // A Task has at most MAX_CHILDREN immediate roots; descendants carry their
    // own sweep. Do not recursively reap Tasks or clear all receipts here.
    let mut at = 0;
    while let Some(child) = task.heir_node(at) {
        at += 1;
        request(&child.node, TaskExitCause::Cascade, super::EXIT_CASCADE);
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
