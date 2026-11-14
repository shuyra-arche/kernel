//! Normal shutdown: observe progress, collect Harts, then finalize exactly once.
//! Entry installs reporting effects; this owner supplies coherent observations.
mod progress;
use crate::basis::unit::{
    lifecycle::tally,
    task::{Task, registry},
};
use crate::metal::{
    clock, hart,
    sync::{OnceLock, SpinLock},
};
use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};
use kernel_api::model::identity::TaskId;
use progress::Progress;

const BARRIER_REPORT_AT: usize = 20_000_000;
static ROOT_ID: AtomicUsize = AtomicUsize::new(0);
static PROGRESS: SpinLock<Progress> = SpinLock::new(Progress::Idle);
static BARRIER: Barrier = Barrier {
    leader: AtomicBool::new(false),
    arrived: AtomicUsize::new(0),
};
struct Barrier {
    leader: AtomicBool,
    arrived: AtomicUsize,
}
enum Arrival {
    Leader,
    Follower,
}
impl Barrier {
    /// Each Hart reaches this once, through the non-returning finish path.
    fn arrive(&self) -> Arrival {
        self.arrived.fetch_add(1, Ordering::AcqRel);
        if self.leader.swap(true, Ordering::AcqRel) {
            Arrival::Follower
        } else {
            Arrival::Leader
        }
    }
    fn count(&self) -> usize {
        self.arrived.load(Ordering::Acquire)
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Observation {
    pub hart: hart::HartId,
    pub pushed: usize,
    pub reaped: usize,
}
type Hook = fn();
struct Hooks {
    finish: &'static [Hook],
    waiting: fn(),
    stalled: fn(Observation),
}
static HOOKS: OnceLock<Hooks> = OnceLock::new();
pub(crate) fn hook(finish: &'static [Hook], waiting: fn(), stalled: fn(Observation)) {
    assert!(
        HOOKS
            .set(Hooks {
                finish,
                waiting,
                stalled
            })
            .is_ok(),
        "shutdown hooks already installed"
    );
}
pub(crate) fn watch(root: &Arc<Task>) {
    ROOT_ID.store(root.ident.id.get(), Ordering::Relaxed);
}
pub(crate) fn active() -> bool {
    let id = ROOT_ID.load(Ordering::Relaxed);
    id != 0
        && registry::find(TaskId::new(id))
            .and_then(|w| w.upgrade())
            .is_none_or(|t| t.dead())
}
pub(crate) fn observe(hart: hart::HartId) {
    if !active() {
        return;
    }
    #[cfg(feature = "latency")]
    crate::metal::measure::stopping();
    // Counters, timestamp and the one-shot report decision share one owner.
    let observation = {
        let mut progress = PROGRESS.lock();
        let (pushed, reaped) = tally::counts();
        let now = clock::now().as_ticks();
        let limit = clock::duration_to_ticks(Duration::from_millis(2_000));
        progress
            .observe(now, (pushed, reaped), limit)
            .then_some(Observation {
                hart,
                pushed,
                reaped,
            })
    };
    if let Some(observation) = observation
        && let Some(hooks) = HOOKS.get()
    {
        (hooks.stalled)(observation);
    }
}
pub(crate) fn barrier() -> (usize, usize) {
    (BARRIER.count(), hart::hart_count())
}
/// The elected leader waits for every Hart, finalizes and resets. Followers stop.
pub(super) fn finish() -> ! {
    crate::metal::mmu::identity::vacate();
    if let Arrival::Leader = BARRIER.arrive() {
        super::scheduler::idle::yell();
        let mut spins = 0;
        while BARRIER.count() < hart::hart_count() {
            spins += 1;
            if spins == BARRIER_REPORT_AT
                && let Some(hooks) = HOOKS.get()
            {
                (hooks.waiting)();
            }
            core::hint::spin_loop();
        }
        #[cfg(feature = "latency")]
        crate::metal::measure::report();
        if let Some(hooks) = HOOKS.get() {
            for finish in hooks.finish {
                finish();
            }
        }
        crate::metal::stop::reset();
    }
    loop {
        hart::wait();
    }
}
