mod queue;
use crate::metal::sync::OnceLock;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use core::time::Duration;
use queue::Queue;

use crate::metal::clock;
use crate::metal::clock::Instant;
use crate::metal::clock::timer;
use crate::metal::hart::{self, HartId};
use crate::metal::layout::MAX_HART_SLOTS;
use crate::metal::sync::Level;
use crate::metal::sync::SpinLock;

// The boot Hart remains online until all Tasks exit. Only it arms and
// drains the global heap; other comparators serve local scheduling.
const OWNER: HartId = HartId::new(0);
static LOCAL: [AtomicU64; MAX_HART_SLOTS] =
    [const { AtomicU64::new(crate::metal::generic::clock::DISARMED) }; MAX_HART_SLOTS];

pub const BLIND_MS: u64 = 100;

pub fn blind_deadline() -> u64 {
    clock::now().add(Duration::from_millis(BLIND_MS)).as_ticks()
}

static LATE_N: AtomicU64 = AtomicU64::new(0);
static LATE_SUM: AtomicU64 = AtomicU64::new(0);
static LATE_MAX: AtomicU64 = AtomicU64::new(0);

static TOCK_N: AtomicU64 = AtomicU64::new(0);
static MUTE_N: AtomicU64 = AtomicU64::new(0);

static TICKS: AtomicU64 = AtomicU64::new(0);
static REMOTE_REARMS: AtomicU64 = AtomicU64::new(0);

fn timers() -> &'static SpinLock<Queue> {
    static TIMERS: OnceLock<SpinLock<Queue>> = OnceLock::new();
    TIMERS.get_or_init(|| SpinLock::new_level(Level::L3, Queue::new()))
}

pub fn elapsed() -> bool {
    if !timer::elapsed() {
        return false;
    }
    TICKS.fetch_add(1, Ordering::Relaxed);
    true
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn is_owner() -> bool {
    hart::hart_id() == OWNER
}

pub fn arm(local: u64) {
    LOCAL[hart::hart_id().get()].store(local, Ordering::Relaxed);
    refresh();
}

/// Acknowledge the software interrupt before calling this on the owner.
/// A later heap mutation then either appears in this snapshot or leaves an IPI.
pub fn refresh() {
    let me = hart::hart_id();
    let local = LOCAL[me.get()].load(Ordering::Relaxed);
    let next = if me == OWNER {
        timers()
            .lock()
            .nearest()
            .map_or(local, |head| head.min(local))
    } else {
        local
    };
    timer::set_timer(next);
}

fn head_changed(changed: bool) {
    if changed {
        if is_owner() {
            refresh();
        } else {
            crate::metal::interrupt::ipi::send(OWNER).expect("deadline owner IPI");
            REMOTE_REARMS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn tock(handle: u64, wake_at: u64) -> Result<(), ()> {
    let mut queue = timers().lock();
    let before = queue.nearest();
    queue.insert(handle, wake_at)?;
    let changed = queue.nearest() != before;
    drop(queue);
    head_changed(changed);
    TOCK_N.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn mute(handle: u64) {
    let mut queue = timers().lock();
    let before = queue.nearest();
    if !queue.remove(handle) {
        return;
    }
    MUTE_N.fetch_add(1, Ordering::Relaxed);
    let changed = queue.nearest() != before;
    drop(queue);
    head_changed(changed);
}

pub fn drain(now: Instant, out: &mut [u64]) -> usize {
    debug_assert!(is_owner());
    let mut n = 0usize;
    let mut i = timers().lock();
    let now = now.as_ticks();
    while n < out.len() {
        let Some((wake_at, handle)) = i.pop_due(now) else {
            break;
        };
        out[n] = handle;
        n += 1;
        let late = now.saturating_sub(wake_at);
        #[cfg(feature = "latency")]
        crate::metal::measure::record(crate::metal::measure::Metric::TimerLate, late);
        LATE_N.fetch_add(1, Ordering::Relaxed);
        LATE_SUM.fetch_add(late, Ordering::Relaxed);
        LATE_MAX.fetch_max(late, Ordering::Relaxed);
    }
    n
}

pub fn late_stats() -> (u64, u64, u64) {
    (
        LATE_N.load(Ordering::Relaxed),
        LATE_MAX.load(Ordering::Relaxed),
        LATE_SUM.load(Ordering::Relaxed),
    )
}

pub fn tock_stats() -> (u64, u64, u64) {
    (
        TOCK_N.load(Ordering::Relaxed),
        MUTE_N.load(Ordering::Relaxed),
        REMOTE_REARMS.load(Ordering::Relaxed),
    )
}
