//! One Hart's execution slot. All entry/departure paths coordinate Task state,
//! queue membership and identity publication under COMMIT and this slot's lock.
pub(crate) mod identity;
mod ready;
mod turn;

use crate::basis::{
    CommitGuard,
    room::deadline,
    sign::{Ticket, WakeKey},
    unit::task::{Task, TaskExitCause, TaskState, WaitCall},
};
use crate::metal::{
    clock,
    hart::HartId,
    switcher::{
        context::{Gprs, TrapContext},
        stack::trap_stack_edge,
    },
    sync::{Level, SpinLock},
};
use alloc::sync::Arc;
use identity::Publication;
use ready::Ready;
use turn::{Cause, Choice, Turn};

const QUANTUM_TICKS: u32 = 8;
struct Inner {
    running: Option<Arc<Task>>,
    ready: Ready,
}
#[repr(align(64))]
pub(crate) struct Slot {
    pub(super) hart: HartId,
    inner: SpinLock<Inner>,
    identity: Publication,
}
/// Required destination state when a current Task leaves this scheduler.
/// Ready/Park are internal scheduling choices; Wait/Exit cross owner boundaries.
pub(crate) enum Departure {
    Wait {
        key: WakeKey,
        ticket: Ticket,
        call: Option<WaitCall>,
    },
    Exit {
        reason: usize,
    },
}
impl Slot {
    pub(super) fn new(hart: HartId) -> Self {
        Self {
            hart,
            inner: SpinLock::new_level(
                Level::Scheduler,
                Inner {
                    running: None,
                    ready: Ready::new(),
                },
            ),
            identity: Publication::new(),
        }
    }
    pub(super) fn enqueue(&self, task: Arc<Task>) -> bool {
        let _commit = crate::basis::commit();
        let mut inner = self.inner.lock();
        let notify = inner.ready.is_empty();
        inner.ready.enqueue(task, clock::now().as_ticks());
        notify
    }
    fn select(inner: &mut Inner, destination: HartId) -> Option<Arc<Task>> {
        for _ in 0..4 {
            let Some(task) = inner.ready.dequeue(destination, clock::now().as_ticks()) else {
                break;
            };
            if task.stopped() || task.ident.team.paused() {
                Task::park(task);
            } else {
                return Some(task);
            }
        }
        if !inner.ready.is_empty() {
            crate::basis::room::deadline::retry();
        }
        None
    }
    pub(super) fn dequeue(&self, _commit: &CommitGuard) -> Option<Arc<Task>> {
        Self::select(&mut self.inner.lock(), self.hart)
    }
    pub(super) fn steal(&self, destination: HartId, _commit: &CommitGuard) -> Option<Arc<Task>> {
        Self::select(&mut *self.inner.try_lock()?, destination)
    }
    pub(super) fn remove(&self, task: &Arc<Task>, _commit: &CommitGuard) -> bool {
        self.inner.lock().ready.remove(task)
    }
    pub(super) fn clear(&self, _commit: &CommitGuard) {
        self.inner.lock().ready.clear();
    }

    fn enter(&self, inner: &mut Inner, task: Arc<Task>, _commit: &CommitGuard) -> usize {
        assert!(inner.running.is_none(), "occupied execution slot");
        assert!(task.can_run_on(self.hart), "incompatible Task placement");
        assert!(
            !task.stopped() && !task.ident.team.paused(),
            "stopped Task admission"
        );
        task.transform_checked(TaskState::running(self.hart, QUANTUM_TICKS));
        let pa = task.ident.frame_pa.as_usize();
        unsafe {
            let frame = &mut *(pa as *mut TrapContext);
            frame.kernel_sp = trap_stack_edge(self.hart);
            if task.ident.team.nest.asid().is_kernel() {
                frame
                    .gpr
                    .set_x(Gprs::TP, crate::metal::hart::per_hart_ptr(self.hart));
            }
        }
        self.identity.enter(&task.ident);
        inner.running = Some(task);
        deadline::arm(inner.ready.deadline(clock::now().as_ticks()));
        pa
    }
    fn leave(&self, inner: &mut Inner, _commit: &CommitGuard) -> Arc<Task> {
        let task = inner.running.take().expect("no running Task");
        assert_eq!(task.on_hart(), Some(self.hart), "execution owner mismatch");
        self.identity.leave(&task.ident);
        task
    }
    pub(super) fn dispatch(&self, task: Arc<Task>, commit: &CommitGuard) -> Option<usize> {
        let mut inner = self.inner.lock();
        if task.stopped() || task.ident.team.paused() {
            Task::park(task);
            return None;
        }
        Some(self.enter(&mut inner, task, commit))
    }
    /// No caller can receive an off-slot Task that still claims this Hart.
    /// A waiting caller retains COMMIT through queue publication. Exit commits
    /// the off-Hart Doomed state here, before lifecycle cleanup continues.
    pub(crate) fn handoff(
        &self,
        departure: Departure,
        commit: &CommitGuard,
    ) -> (Arc<Task>, Option<usize>) {
        let mut inner = self.inner.lock();
        let task = self.leave(&mut inner, commit);
        match departure {
            Departure::Wait { key, ticket, call } => task.block(key, ticket, call),
            Departure::Exit { reason } => {
                let mut state = task.state.lock();
                let (cause, reason) = state.dying().unwrap_or((TaskExitCause::Reap, reason));
                state.doom(None, cause, reason);
            }
        }
        let next =
            Self::select(&mut inner, self.hart).map(|next| self.enter(&mut inner, next, commit));
        (task, next)
    }
    pub(crate) fn running_task(&self) -> Option<Arc<Task>> {
        self.inner.lock().running.as_ref().map(Arc::clone)
    }
    fn step(&self, cause: Cause, commit: &CommitGuard) -> Option<usize> {
        let mut inner = self.inner.lock();
        let cur = inner.running.as_ref()?;
        let now = clock::now().as_ticks();
        let ceiling = inner.ready.deadline(now);
        let choice = Turn {
            stopped: cur.stopped() || cur.ident.team.paused(),
            ready: !inner.ready.is_empty(),
            remaining: cur.ticks_left(),
            now,
            ceiling,
        }
        .decide(cause);
        match choice {
            Choice::Continue { spend, arm } => {
                if spend {
                    cur.dec_ticks_left();
                }
                if arm {
                    deadline::arm(ceiling);
                }
                Some(cur.ident.frame_pa.as_usize())
            }
            Choice::Rotate | Choice::Park => {
                let cur = self.leave(&mut inner, commit);
                if choice == Choice::Park {
                    Task::park(cur);
                } else {
                    cur.transform_checked(TaskState::ready());
                    inner.ready.enqueue(cur, now);
                }
                Self::select(&mut inner, self.hart).map(|next| self.enter(&mut inner, next, commit))
            }
        }
    }
    pub(super) fn advance(&self, commit: &CommitGuard) -> Option<usize> {
        self.step(Cause::Tick, commit)
    }
    pub(crate) fn reschedule(&self) -> usize {
        let commit = crate::basis::commit();
        if let Some(route) = crate::basis::room::interrupt::external() {
            route.retry(&commit);
        }
        if self.running_task().is_some_and(|t| t.doomed()) {
            drop(commit);
            return crate::basis::unit::lifecycle::quit();
        }
        let next = self.step(Cause::Yield, &commit);
        drop(commit);
        next.unwrap_or_else(super::idle::fetch)
    }
}
pub(crate) fn current() -> &'static Slot {
    unsafe { &*(crate::metal::hart::scheduler() as *const Slot) }
}
