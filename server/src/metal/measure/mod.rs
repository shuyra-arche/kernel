//! Opt-in latency observations. No allocation, locking, formatting or retry loop
//! occurs in a sample; storage is allocated once before secondary harts start.
use crate::metal::{clock, hart, sync::OnceLock};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
mod distribution;
pub(crate) mod execution;
mod transaction;
use core::cell::UnsafeCell;
use distribution::Distribution;
pub(crate) use transaction::Transaction;
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Metric {
    IrqOff,
    Kernel,
    SpinWait,
    SpinHold,
    CommitWait,
    CommitHold,
    IrqSign,
    ReadyRun,
    TimerLate,
    Probe,
    OuterWait,
    OuterHold,
    ReadyQueue,
    DispatchRun,
}
const NAMES: [&str; 14] = [
    "irq-off",
    "kernel-active",
    "spin-wait",
    "spin-hold",
    "commit-wait",
    "commit-hold",
    "irq-sign",
    "ready-run",
    "timer-late",
    "probe",
    "outer-wait",
    "outer-hold",
    "ready-queue",
    "dispatch-run",
];
#[repr(align(64))]
struct Local {
    metrics: [Distribution; 14],
    slowest: UnsafeCell<transaction::Slowest>,
    kernel: AtomicU64,
    irq: AtomicU64,
}
// Only this Hart writes attribution while COMMIT and its IRQ guard are held.
// Reporting runs after every Hart has reached the shutdown barrier.
unsafe impl Sync for Local {}
impl Local {
    const fn new() -> Self {
        Self {
            metrics: [const { Distribution::new() }; 14],
            slowest: UnsafeCell::new(transaction::Slowest::new()),
            kernel: AtomicU64::new(0),
            irq: AtomicU64::new(0),
        }
    }
}
static LOCALS: OnceLock<Box<[Local]>> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) fn init() -> Result<(), crate::basis::nest::NestError> {
    let mut storage = Box::<[Local]>::try_new_uninit_slice(hart::hart_count())
        .map_err(|_| crate::basis::nest::NestError::OutOfMemory)?;
    for slot in storage.iter_mut() {
        slot.write(Local::new());
    }
    LOCALS
        .set(unsafe { storage.assume_init() })
        .map_err(|_| crate::basis::nest::NestError::OutOfMemory)?;
    ACTIVE.store(true, Ordering::Release);
    // Calibration floor: two clock reads and one same-layout distribution update.
    let probe = Distribution::new();
    for _ in 0..1024 {
        let start = stamp();
        probe.record(0);
        elapsed(Metric::Probe, start);
    }
    Ok(())
}
pub(crate) fn stamp() -> u64 {
    if ACTIVE.load(Ordering::Relaxed) {
        clock::now().as_ticks().max(1)
    } else {
        0
    }
}
pub(crate) fn record(metric: Metric, ticks: u64) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        local.metrics[metric as usize].record(ticks);
    }
}
pub(crate) fn elapsed(metric: Metric, start: u64) {
    if start != 0 {
        record(metric, clock::now().as_ticks().saturating_sub(start));
    }
}
pub(crate) fn enter() {
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        local.kernel.store(stamp(), Ordering::Relaxed);
        local.irq.store(stamp(), Ordering::Relaxed);
    }
}
pub(crate) fn leave() {
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        let start = local.kernel.swap(0, Ordering::Relaxed);
        elapsed(Metric::Kernel, start);
        elapsed(Metric::IrqOff, local.irq.swap(0, Ordering::Relaxed));
    }
}
pub(crate) fn pause_irq() -> bool {
    let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) else {
        return false;
    };
    let start = local.irq.swap(0, Ordering::Relaxed);
    elapsed(Metric::IrqOff, start);
    start != 0
}
pub(crate) fn resume_irq(measured: bool) {
    if measured && let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        local.irq.store(stamp(), Ordering::Relaxed);
    }
}
pub(crate) fn transaction(
    start: u64,
    site: &'static core::panic::Location<'static>,
) -> Transaction {
    let now = stamp();
    let wait = now.saturating_sub(start);
    if start != 0 {
        record(Metric::OuterWait, wait);
    }
    Transaction {
        site,
        start: now,
        wait,
    }
}
pub(crate) fn complete(transaction: Transaction) {
    if transaction.start == 0 || !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let hold = stamp().saturating_sub(transaction.start);
    record(Metric::OuterHold, hold);
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        // Called only by the last COMMIT guard, before releasing ownership.
        unsafe { &mut *local.slowest.get() }.record(transaction::Sample {
            site: transaction.site,
            hold,
            wait: transaction.wait,
        });
    }
}
pub(crate) fn report() {
    let (opened, captured) = crate::metal::interrupt::window::stats();
    crate::putln!("latency-windows: opened={} captured={}", opened, captured);
    ACTIVE.store(false, Ordering::Release);
    let Some(all) = LOCALS.get() else {
        return;
    };
    crate::putln!(
        "latency: ticks-hz={} bins=log2-saturated-32 proof=observation",
        crate::entry::boot::facts::info().hart.hertz()
    );
    for (hart, local) in all.iter().enumerate() {
        // All writers are quiescent at the shutdown barrier.
        for sample in unsafe { &*local.slowest.get() }.samples.iter().flatten() {
            crate::putln!(
                "latency-outer: hart={} hold={} wait={} site={}:{}:{}",
                hart,
                sample.hold,
                sample.wait,
                sample.site.file(),
                sample.site.line(),
                sample.site.column()
            );
        }
        for (name, metric) in NAMES.iter().zip(&local.metrics) {
            let (count, max, bins) = metric.snapshot();
            crate::putln!(
                "latency: hart={} metric={} count={} max={} bins={:?}",
                hart,
                name,
                count,
                max,
                bins
            );
        }
    }
}
