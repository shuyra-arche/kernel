//! Ready membership and the queue's scheduling ceiling change together.
use crate::basis::unit::task::{Task, chain::Chain};
use crate::metal::{clock, hart::HartId};
use alloc::sync::Arc;
use core::time::Duration;

pub(super) struct Ready {
    chain: Chain,
    since: Option<u64>,
}
impl Ready {
    pub(super) const fn new() -> Self {
        Self {
            chain: Chain::new(),
            since: None,
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }
    pub(super) fn deadline(&self, now: u64) -> u64 {
        let (start, ms) = self
            .since
            .map_or((now, crate::basis::room::deadline::BLIND_MS), |since| {
                (since, 2)
            });
        start.saturating_add(clock::duration_to_ticks(Duration::from_millis(ms)))
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
        let empty = self.chain.is_empty();
        self.chain.enqueue(task);
        if empty {
            self.since = Some(now);
        }
    }
    pub(super) fn dequeue(&mut self, destination: HartId, now: u64) -> Option<Arc<Task>> {
        for _ in 0..4 {
            let task = self.chain.dequeue()?;
            if task.can_run_on(destination) {
                self.since = (!self.chain.is_empty()).then_some(now);
                return Some(task);
            }
            self.chain.enqueue(task);
        }
        crate::basis::room::deadline::retry();
        None
    }
    pub(super) fn remove(&mut self, target: &Arc<Task>) -> bool {
        let removed = self.chain.remove(target).is_some();
        if self.chain.is_empty() {
            self.since = None;
        }
        removed
    }
    pub(super) fn clear(&mut self) {
        while let Some(task) = self.chain.dequeue() {
            if task.stopped() || task.ident.team.paused() {
                Task::park(task);
            }
        }
        self.since = None;
    }
}
