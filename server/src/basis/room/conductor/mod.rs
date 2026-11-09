//! 运行槽：每核一张。
//!
//! 槽里装着此刻在座的那一枚任务、它的饥饿队列（FIFO），以及队列起等的时刻 ——
//! 队列等久了就压低本核的时限（`ready_deadline`），把核让给别的活。
//! 跨核的事不在这里：往哪放、怎么叫醒，都归 `scheduler`。

mod badge;
pub(crate) mod identity;
pub(crate) mod run;

use alloc::sync::Arc;
use core::time::Duration;

use crate::basis::room::chain::Chain;
use crate::basis::room::deadline;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskState;
use crate::metal::clock;
use crate::metal::hart::HartId;
use crate::metal::switcher::context::Gprs;
use crate::metal::switcher::context::TrapContext;
use crate::metal::switcher::stack::trap_stack_edge;
use crate::metal::sync::Level;
use crate::metal::sync::SpinLock;

use self::badge::Badge;

const QUANTUM_TICKS: u32 = 8;
const READY_MS: u64 = 2;

// 槽里被锁住的那部分：在座任务、饥饿队列、队列起等的时刻。

pub(super) struct Inner {
    pub(super) running: Option<Arc<Task>>,
    pub(super) chain: Chain,
    pub(super) ready_since: Option<u64>,
}

impl Inner {
    /// 本核这一段能跑多久：队列空着按盲上限，等着了按起等时刻折算出余量。
    pub(super) fn ready_deadline(&self) -> u64 {
        self.ready_since
            .map_or_else(deadline::blind_deadline, |since| {
                since.saturating_add(clock::duration_to_ticks(Duration::from_millis(READY_MS)))
            })
    }

    pub(super) fn starved_is_empty(&self) -> bool {
        self.chain.is_empty()
    }
}

#[repr(align(64))]
pub(crate) struct Conductor {
    pub(super) hart: HartId,
    pub(super) inner: SpinLock<Inner>,
    pub(super) badge: Badge,
}

impl Conductor {
    pub(super) fn new(hart: HartId) -> Conductor {
        Conductor {
            hart,
            inner: SpinLock::new_level(
                Level::Scheduler,
                Inner {
                    running: None,
                    chain: Chain::new(),
                    ready_since: None,
                },
            ),
            badge: Badge::new(),
        }
    }

    fn starved_push(&self, i: &mut Inner, task: Arc<Task>) {
        debug_assert!(
            task.ready() && task.next().is_none(),
            "starved 容器只收就绪任务，且入队前不得挂在链上"
        );
        if i.chain.is_empty() {
            i.ready_since = Some(clock::now().as_ticks());
        }
        i.chain.push(task);
    }

    fn starved_pop(&self, i: &mut Inner, destination: HartId) -> Option<Arc<Task>> {
        loop {
            let head = i
                .chain
                .remove_if(&mut |task| task.can_run_on(destination))?;
            i.ready_since = if i.chain.is_empty() {
                None
            } else {
                Some(clock::now().as_ticks())
            };
            if head.stopped() || head.ident.team.paused() {
                Task::park(head);
                continue;
            }
            return Some(head);
        }
    }

    pub(super) fn starved_remove(&self, i: &mut Inner, target: &Arc<Task>) -> bool {
        let removed = i.chain.remove_if(&mut |t| Arc::ptr_eq(t, target)).is_some();
        if removed && i.chain.is_empty() {
            i.ready_since = None;
        }
        removed
    }

    pub(super) fn starved_clear(&self, i: &mut Inner) {
        let _commit = crate::basis::commit();
        while let Some(task) = i.chain.pop() {
            if task.stopped() || task.ident.team.paused() {
                Task::park(task);
            }
        }
        i.ready_since = None;
    }

    pub(super) fn push(&self, task: Arc<Task>) -> bool {
        let _commit = crate::basis::commit();
        debug_assert!(task.ready(), "starved 容器只收就绪任务");
        let mut i = self.inner.lock();
        let notify = i.starved_is_empty();
        self.starved_push(&mut i, task);
        notify
    }

    fn pull(&self, _commit: &crate::basis::CommitGuard) -> Option<Arc<Task>> {
        // dispatch holds COMMIT until this Task is seated on a Hart.
        // Reacquiring it here adds work but supplies no additional exclusion.
        #[cfg(debug_assertions)]
        crate::basis::assert_held();
        let mut i = self.inner.lock();
        self.starved_pop(&mut i, self.hart)
    }

    pub(super) fn steal(&self, destination: HartId) -> Option<Arc<Task>> {
        let _commit = crate::basis::commit();
        let mut i = self.inner.try_lock()?;
        self.starved_pop(&mut i, destination)
    }

    fn prepare(&self, task: &mut Arc<Task>, ceiling: u64) {
        let t = task.as_ref();
        t.transform_checked(TaskState::running(self.hart, QUANTUM_TICKS));
        unsafe {
            let frame = &mut *(t.ident.frame_pa.as_usize() as *mut TrapContext);
            frame.kernel_sp = trap_stack_edge(self.hart);
            if t.ident.team.nest.asid().is_kernel() {
                frame
                    .gpr
                    .set_x(Gprs::TP, crate::metal::hart::per_hart_ptr(self.hart));
            }
        }
        deadline::arm(ceiling);
    }

    fn seat(&self, mut task: Arc<Task>, _commit: &crate::basis::CommitGuard) -> Option<usize> {
        // Callers retain COMMIT across dequeue/steal and seating: a Ready Task
        // must not escape every scheduler container between those transitions.
        #[cfg(debug_assertions)]
        crate::basis::assert_held();
        let mut i = self.inner.lock();
        if task.stopped() || task.ident.team.paused() {
            Task::park(task);
            return None;
        }
        assert!(task.can_run_on(self.hart), "incompatible Task placement");
        self.prepare(&mut task, i.ready_deadline());
        let pa = task.ident.frame_pa.as_usize();
        self.badge.seat(&task.ident);
        debug_assert!(task.on_hart().is_some(), "running 容器只装在核上的任务");
        let prev = i.running.replace(task);
        debug_assert!(prev.is_none(), "装槽前 running 必须为空");
        Some(pa)
    }

    pub(crate) fn swap(&self) -> (Arc<Task>, Option<usize>) {
        let _commit = crate::basis::commit();
        let mut i = self.inner.lock();
        let task = i.running.take().expect("no running task");
        let next = self.starved_pop(&mut i, self.hart);
        drop(i);
        self.badge.shed(&task.ident);
        let next_pa = if let Some(next) = next {
            self.seat(next, &_commit)
        } else {
            None
        };
        (task, next_pa)
    }

    pub(crate) fn running_task(&self) -> Option<Arc<Task>> {
        let i = self.inner.lock();
        i.running.as_ref().map(Arc::clone)
    }

    fn rotate(&self, i: &mut Inner, cur: Arc<Task>) -> Option<Arc<Task>> {
        cur.transform_checked(TaskState::ready());
        self.starved_push(i, cur);
        self.starved_pop(i, self.hart)
    }

    pub(crate) fn starve(&self) -> usize {
        let _commit = crate::basis::commit();
        if self.running_task().is_some_and(|t| t.doomed()) {
            drop(_commit);
            return crate::basis::unit::lifecycle::quit();
        }
        if self
            .running_task()
            .is_some_and(|t| t.stopped() || t.ident.team.paused())
        {
            let next = self.advance();
            drop(_commit);
            return next.unwrap_or_else(run::fetch);
        }
        let mut i = self.inner.lock();
        let Some(cur) = i.running.take() else {
            panic!("starve with no running task on hart {}", self.hart);
        };
        if i.starved_is_empty() {
            let pa = cur.ident.frame_pa.as_usize();
            i.running = Some(cur);
            return pa;
        }
        self.badge.shed(&cur.ident);
        let next = self.rotate(&mut i, cur);
        drop(i);
        let pa = next.and_then(|next| self.seat(next, &_commit));
        drop(_commit);
        pa.unwrap_or_else(run::fetch)
    }

    fn advance(&self) -> Option<usize> {
        let _commit = crate::basis::commit();
        let mut i = self.inner.lock();
        let cur = i.running.take()?;
        if cur.stopped() || cur.ident.team.paused() {
            self.badge.shed(&cur.ident);
            Task::park(cur);
            let next = self.starved_pop(&mut i, self.hart);
            drop(i);
            return next.and_then(|next| self.seat(next, &_commit));
        }
        let ticks_left = cur.ticks_left();
        let ceiling = i.ready_deadline();
        if i.starved_is_empty() || (ticks_left > 1 && clock::now().as_ticks() < ceiling) {
            if ticks_left > 1 {
                cur.dec_ticks_left();
            }
            let pa = cur.ident.frame_pa.as_usize();
            i.running = Some(cur);
            deadline::arm(ceiling);
            return Some(pa);
        }
        self.badge.shed(&cur.ident);
        let next = self.rotate(&mut i, cur);
        drop(i);
        let pa = next.and_then(|next| self.seat(next, &_commit));
        pa
    }
}

/// 本核的运行槽。
pub(crate) fn current() -> &'static Conductor {
    unsafe { &*(crate::metal::hart::scheduler() as *const Conductor) }
}
