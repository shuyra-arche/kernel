//! Ordinary rotation and the admitted fixed-priority set share Task membership.
use crate::basis::unit::task::{Task, chain::Chain, schedule::account::MAX_TASKS};
use crate::metal::{clock, hart::HartId};
use alloc::sync::Arc;
use core::time::Duration;

pub(super) struct Ready {
    ordinary: Chain,
    fixed: Chain,
    since: Option<u64>,
}
impl Ready {
    pub(super) const fn new() -> Self {
        Self {
            ordinary: Chain::new(),
            fixed: Chain::new(),
            since: None,
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.ordinary.is_empty() && self.fixed.is_empty()
    }
    pub(super) fn ordinary(&self) -> bool {
        !self.ordinary.is_empty()
    }
    pub(super) fn deadline(&self, now: u64, priority: u8) -> u64 {
        let mut at = now.saturating_add(clock::duration_to_ticks(Duration::from_millis(
            crate::basis::room::deadline::BLIND_MS,
        )));
        let mut peer = priority == 0 && !self.ordinary.is_empty();
        self.fixed.visit(MAX_TASKS, |task| {
            let mut schedule = task.schedule.lock();
            if schedule.available(now) {
                peer |= schedule.priority() >= priority;
            } else {
                at = at.min(schedule.deadline(now));
            }
        });
        // A lower-priority waiter cannot impose its expired rotation deadline
        // on the current Task: doing so causes a past-comparator interrupt storm.
        if peer && let Some(since) = self.since {
            at = at.min(since.saturating_add(clock::duration_to_ticks(Duration::from_millis(2))));
        }
        at
    }
    pub(super) fn priority(&self, now: u64) -> u8 {
        let mut best = 0;
        self.fixed.visit(MAX_TASKS, |task| {
            let mut schedule = task.schedule.lock();
            if schedule.available(now) {
                best = best.max(schedule.priority());
            }
        });
        best
    }
    pub(super) fn enqueue(&mut self, task: Arc<Task>, now: u64) {
        assert!(task.ready(), "ready queue requires a Ready Task");
        #[cfg(feature = "latency")]
        let _ = task.ready_stamp.compare_exchange(
            0,
            crate::metal::measure::stamp(),
            core::sync::atomic::Ordering::Relaxed,
            core::sync::atomic::Ordering::Relaxed,
        );
        let empty = self.is_empty();
        if task.realtime() {
            assert!(self.fixed.len() < MAX_TASKS);
            self.fixed.enqueue(task);
        } else {
            self.ordinary.enqueue(task);
        }
        if empty {
            self.since = Some(now);
        }
    }
    pub(super) fn dequeue(&mut self, destination: HartId, now: u64) -> Option<Arc<Task>> {
        let mut best: Option<Arc<Task>> = None;
        let mut priority = 0;
        self.fixed.visit(MAX_TASKS, |task| {
            if !task.can_run_on(destination) {
                return;
            }
            let mut schedule = task.schedule.lock();
            if schedule.available(now) && schedule.priority() > priority {
                priority = schedule.priority();
                best = Some(task.clone());
            }
        });
        if let Some(task) = best {
            let selected = self.fixed.remove(&task);
            self.since = (!self.is_empty()).then_some(now);
            return selected;
        }
        for _ in 0..4 {
            let task = self.ordinary.dequeue()?;
            if task.can_run_on(destination) {
                self.since = (!self.is_empty()).then_some(now);
                return Some(task);
            }
            self.ordinary.enqueue(task);
        }
        crate::basis::room::deadline::retry();
        None
    }
    pub(super) fn relocate(&mut self) -> Option<Arc<Task>> {
        self.ordinary.dequeue()
    }
    pub(super) fn remove(&mut self, target: &Arc<Task>) -> bool {
        let removed = if target.realtime() {
            self.fixed.remove(target)
        } else {
            self.ordinary.remove(target)
        }
        .is_some();
        if self.is_empty() {
            self.since = None;
        }
        removed
    }
    pub(super) fn clear(&mut self) {
        for chain in [&mut self.ordinary, &mut self.fixed] {
            while let Some(task) = chain.dequeue() {
                if task.stopped() || task.ident.team.paused() {
                    Task::park(task);
                }
            }
        }
        self.since = None;
    }
}
