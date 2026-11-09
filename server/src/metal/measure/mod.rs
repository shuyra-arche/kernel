//! Opt-in latency observations. No allocation, locking, formatting or retry loop
//! occurs in a sample; storage is allocated once before secondary harts start.
use crate::metal::{clock, hart, sync::OnceLock};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
mod distribution;
use distribution::Distribution;
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
}
const NAMES: [&str; 10] = [
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
];
#[repr(align(64))]
struct Local {
    metrics: [Distribution; 10],
    kernel: AtomicU64,
    irq: AtomicU64,
}
impl Local {
    const fn new() -> Self {
        Self {
            metrics: [const { Distribution::new() }; 10],
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
