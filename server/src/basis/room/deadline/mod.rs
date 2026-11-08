mod queue;
use crate::machine::sync::OnceLock;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use core::time::Duration;
use queue::Queue;

use crate::machine::clock;
use crate::machine::clock::Instant;
use crate::machine::sync::Level;
use crate::machine::sync::SpinLock;

const NONE: u64 = u64::MAX;

pub const BLIND_MS: u64 = 100;

pub fn blind_ceiling() -> u64 {
    clock::duration_to_ticks(Duration::from_millis(BLIND_MS))
}

static LATE_N: AtomicU64 = AtomicU64::new(0);
static LATE_SUM: AtomicU64 = AtomicU64::new(0);
static LATE_MAX: AtomicU64 = AtomicU64::new(0);

static TOCK_N: AtomicU64 = AtomicU64::new(0);
static MUTE_N: AtomicU64 = AtomicU64::new(0);

static TICKS: AtomicU64 = AtomicU64::new(0);

struct TimerHeap {
    inner: SpinLock<Queue>,
    nearest: AtomicU64,
}

static TIMERS: OnceLock<TimerHeap> = OnceLock::new();
fn timers() -> &'static TimerHeap {
    TIMERS.get_or_init(|| TimerHeap {
        inner: SpinLock::new_level(Level::L3, Queue::new()),
        nearest: AtomicU64::new(NONE),
    })
}

impl TimerHeap {
    fn peek_nearest(&self) -> u64 {
        self.nearest.load(Ordering::Acquire)
    }

    fn recompute_nearest(&self, i: &Queue) {
        let t = i.nearest().unwrap_or(NONE);
        self.nearest.store(t, Ordering::Release);
    }
}

pub fn tick() -> u64 {
    TICKS.fetch_add(1, Ordering::Relaxed) + 1
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn beat_until(ceil: u64) {
    let delta = match due() {
        Some(t) => t
            .as_ticks()
            .saturating_sub(clock::now().as_ticks())
            .min(ceil),
        None => ceil,
    };
    clock::timer::beat(delta);
}

pub fn tock(handle: u64, wake_at: u64) -> Result<(), ()> {
    let mut i = timers().inner.lock();
    i.insert(handle, wake_at)?;
    timers().recompute_nearest(&i);
    TOCK_N.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub fn mute(handle: u64) {
    let mut i = timers().inner.lock();
    if i.remove(handle) {
        MUTE_N.fetch_add(1, Ordering::Relaxed);
    }
    timers().recompute_nearest(&i);
}

pub fn due() -> Option<Instant> {
    let t = timers().peek_nearest();
    (t != NONE).then_some(Instant::from_ticks(t))
}

pub fn drain(now: Instant, out: &mut [u64]) -> usize {
    let mut n = 0usize;
    let mut i = timers().inner.lock();
    let now = now.as_ticks();
    while n < out.len() {
        let Some((wake_at, handle)) = i.pop_due(now) else {
            break;
        };
        out[n] = handle;
        n += 1;
        let late = now.saturating_sub(wake_at);
        LATE_N.fetch_add(1, Ordering::Relaxed);
        LATE_SUM.fetch_add(late, Ordering::Relaxed);
        LATE_MAX.fetch_max(late, Ordering::Relaxed);
    }
    timers().recompute_nearest(&i);
    n
}

pub fn late_stats() -> (u64, u64, u64) {
    (
        LATE_N.load(Ordering::Relaxed),
        LATE_MAX.load(Ordering::Relaxed),
        LATE_SUM.load(Ordering::Relaxed),
    )
}

pub fn tock_stats() -> (u64, u64) {
    (
        TOCK_N.load(Ordering::Relaxed),
        MUTE_N.load(Ordering::Relaxed),
    )
}
