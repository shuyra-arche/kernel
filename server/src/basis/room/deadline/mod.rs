mod local;
mod queue;
mod retry;
use crate::metal::sync::OnceLock;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
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
static LOCAL: [local::Local; MAX_HART_SLOTS] = [const { local::Local::new() }; MAX_HART_SLOTS];
static RETRIES: [retry::Retry; MAX_HART_SLOTS] = [const { retry::Retry::new() }; MAX_HART_SLOTS];

pub const BLIND_MS: u64 = 100;

pub fn blind_deadline() -> u64 {
    clock::now()
        .add(core::time::Duration::from_millis(BLIND_MS))
        .as_ticks()
}

static LATE_N: AtomicU64 = AtomicU64::new(0);
static LATE_SUM: AtomicU64 = AtomicU64::new(0);
static LATE_MAX: AtomicU64 = AtomicU64::new(0);

static INSERTS: AtomicU64 = AtomicU64::new(0);
static REMOVES: AtomicU64 = AtomicU64::new(0);

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
    RETRIES[hart::hart_id().get()].elapsed(clock::now().as_ticks());
    true
}

/// Retain a timer retry without acquiring the shared heap or changing Task
/// deadlines. The local comparator is the only hardware effect of rejection.
pub(crate) fn retry() {
    let now = clock::now().as_ticks();
    let delay = clock::duration_to_ticks(core::time::Duration::from_micros(100));
    let at = RETRIES[hart::hart_id().get()].request(now, delay);
    timer::retry(now, at);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn is_owner() -> bool {
    hart::hart_id() == OWNER
}

pub fn arm(local: u64) {
    stage(local);
    refresh();
}

/// Scheduler commits retain the desired comparator without entering firmware
/// while global COMMIT or the execution-slot lock is held.
pub(crate) fn stage(local: u64) {
    LOCAL[hart::hart_id().get()].stage(local);
}

/// All Task returns pass this boundary after releasing scheduler/COMMIT guards.
/// A failed heap snapshot retains both staged work and the existing timer retry.
pub(crate) fn flush() {
    #[cfg(debug_assertions)]
    crate::basis::assert_unlocked();
    // Suppress ordinary reception callbacks until the selected value and its
    // hardware effect have settled together. This retains no global lock.
    let _interrupts = unsafe { crate::metal::sync::TrapGuard::save() };
    let _ = LOCAL[hart::hart_id().get()].flush(program);
}

/// Acknowledge the software interrupt before calling this on the owner.
/// A later heap mutation then either appears in this snapshot or leaves an IPI.
pub fn refresh() {
    let _interrupts = unsafe { crate::metal::sync::TrapGuard::save() };
    let _ = LOCAL[hart::hart_id().get()].refresh(program);
}

fn program(local: u64) -> Result<(), ()> {
    let me = hart::hart_id();
    let local = local.min(RETRIES[me.get()].deadline());
    let next = if me == OWNER {
        let Some(queue) = timers().try_lock() else {
            retry();
            return Err(());
        };
        queue.nearest().map_or(local, |head| head.min(local))
    } else {
        local
    };
    timer::set_timer(next);
    Ok(())
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

pub fn insert(handle: u64, wake_at: u64) -> Result<(), ()> {
    let mut queue = timers().lock();
    let before = queue.nearest();
    queue.insert(handle, wake_at)?;
    let changed = queue.nearest() != before;
    drop(queue);
    head_changed(changed);
    INSERTS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn remove(handle: u64) {
    let mut queue = timers().lock();
    let before = queue.nearest();
    if !queue.remove(handle) {
        return;
    }
    REMOVES.fetch_add(1, Ordering::Relaxed);
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

pub fn registration_stats() -> (u64, u64, u64) {
    (
        INSERTS.load(Ordering::Relaxed),
        REMOVES.load(Ordering::Relaxed),
        REMOTE_REARMS.load(Ordering::Relaxed),
    )
}
