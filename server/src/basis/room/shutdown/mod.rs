//! Normal shutdown: observe progress, collect Harts, then finalize exactly once.
//! Entry installs reporting effects; this owner supplies coherent observations.
use crate::basis::sync::{LegacyLock, order};
mod preparation;
mod progress;
mod root;
use crate::basis::unit::{
    lifecycle::tally,
    task::{Task, TaskState},
};
use crate::metal::{clock, hart, sync::OnceLock};
use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};
pub(crate) use preparation::Unsettled;
use preparation::{Claim, Preparation};
use progress::Progress;

const BARRIER_REPORT_AT: usize = 20_000_000;
static ROOT: root::Root = root::Root::new();
static PROGRESS: LegacyLock<crate::basis::sync::order::Shutdown, Progress> =
    LegacyLock::new(Progress::Idle);
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
/// Only Hart convergence, not CPU/IOMMU/external-agent retirement evidence.
/// Constructed after preparation and the final Hart barrier both complete.
pub(crate) struct Converged {
    _private: (),
}
pub(crate) struct Finalization {
    pub prepare: fn() -> Result<(), Unsettled>,
    pub finish: fn(Converged),
}
pub(crate) struct Observers {
    pub waiting: fn(),
    pub stalled: fn(Observation),
    pub finished: fn(Observation),
    pub unsettled: fn(Observation, Unsettled),
}
static FINALIZATION: OnceLock<Finalization> = OnceLock::new();
static OBSERVERS: OnceLock<Observers> = OnceLock::new();
static PREPARATION: Preparation = Preparation::new();
static REPORTED: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
pub(crate) fn bind_finalization(binding: Finalization) -> Result<(), &'static str> {
    FINALIZATION
        .set(binding)
        .map_err(|_| "shutdown finalization already bound")
}
/// Observers must be bounded, synchronous, nonblocking and non-reentrant.
/// They run without COMMIT and receive no owning Task/resource reference.
/// Omitting them changes no mandatory transition or finalization effect.
pub(crate) fn observe_with(observers: Observers) -> Result<(), &'static str> {
    OBSERVERS
        .set(observers)
        .map_err(|_| "shutdown observers already bound")
}
/// Failed/Busy preparation leaves Harts on their scheduler stacks with normal
/// maintenance and completion progress available. It never authorizes a reset.
pub(crate) fn ready() -> bool {
    if !tally::done() {
        return false;
    }
    if !crate::basis::nest::retirement::settle() {
        return false;
    }
    let binding = FINALIZATION
        .get()
        .expect("required shutdown binding before operation");
    match PREPARATION.claim() {
        Claim::Prepare => {
            let result = (binding.prepare)();
            let ready = PREPARATION.complete(result);
            if result == Err(Unsettled::Busy) {
                // Even an image without an initial Task must retain a retry:
                // No root may be watched while queue shutdown is pending.
                crate::basis::room::deadline::retry();
            }
            if let Err(reason) = result {
                let bit = if reason == Unsettled::Busy { 1 } else { 2 };
                if REPORTED.fetch_or(bit, Ordering::Relaxed) & bit == 0
                    && let Some(observers) = OBSERVERS.get()
                {
                    let (pushed, reaped) = tally::counts();
                    (observers.unsettled)(
                        Observation {
                            hart: hart::hart_id(),
                            pushed,
                            reaped,
                        },
                        reason,
                    );
                }
            }
            ready
        }
        Claim::Ready => true,
        Claim::Wait | Claim::Failed => false,
    }
}
pub(crate) fn watch(root: &Arc<Task>) {
    ROOT.watch(&root.state)
        .expect("shutdown root registered once");
}
pub(crate) fn active() -> bool {
    ROOT.active()
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
        && let Some(hooks) = OBSERVERS.get()
    {
        (hooks.stalled)(observation);
    }
}
pub(crate) fn barrier() -> (usize, usize) {
    (BARRIER.count(), hart::hart_count())
}
/// The elected leader waits for every Hart, finalizes and resets. Followers stop.
pub(super) fn finish() -> ! {
    assert!(matches!(PREPARATION.claim(), Claim::Ready));
    crate::metal::mmu::internal::identity::vacate();
    if let Arrival::Leader = BARRIER.arrive() {
        super::scheduler::idle::yell();
        let mut spins = 0;
        while BARRIER.count() < hart::hart_count() {
            spins += 1;
            if spins == BARRIER_REPORT_AT
                && let Some(hooks) = OBSERVERS.get()
            {
                (hooks.waiting)();
            }
            core::hint::spin_loop();
        }
        let (pushed, reaped) = tally::counts();
        let snapshot = Observation {
            hart: hart::hart_id(),
            pushed,
            reaped,
        };
        (FINALIZATION
            .get()
            .expect("required shutdown binding")
            .finish)(Converged { _private: () });
        #[cfg(feature = "latency")]
        crate::metal::measure::report();
        if let Some(hooks) = OBSERVERS.get() {
            (hooks.finished)(snapshot);
        }
        crate::metal::stop::reset();
    }
    loop {
        hart::wait();
    }
}
