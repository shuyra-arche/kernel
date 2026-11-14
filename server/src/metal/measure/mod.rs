//! Opt-in latency observations. No allocation, locking, formatting or retry loop
//! occurs in a sample; storage is allocated once before secondary harts start.
use crate::metal::{clock, hart, sync::OnceLock};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
mod distribution;
pub(crate) mod execution;
mod timeline;
mod transaction;
use core::cell::UnsafeCell;
use distribution::Distribution;
pub(crate) use transaction::Transaction;
pub(crate) use transaction::Work;
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
    IrqAdmit,
    IrqPublish,
    TimerReturn,
    ControllerClaim,
}
const METRICS: usize = 18;
const NAMES: [&str; METRICS] = [
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
    "irq-admit",
    "irq-publish",
    "timer-return",
    "controller-claim",
];
#[repr(align(64))]
struct Local {
    metrics: [Distribution; METRICS],
    phases: [[Distribution; METRICS]; 3],
    slowest: UnsafeCell<transaction::Slowest>,
    phase_slowest: [UnsafeCell<transaction::Slowest>; 3],
    phase_ready: [UnsafeCell<Option<(usize, execution::Sample)>>; 3],
    phase_irq: [UnsafeCell<Option<(&'static core::panic::Location<'static>, u64, u64)>>; 3],
    transaction: AtomicU64,
    parts: UnsafeCell<transaction::Parts>,
    ready: UnsafeCell<Option<(usize, execution::Sample)>>,
    irq_site: UnsafeCell<Option<&'static core::panic::Location<'static>>>,
    irq_transaction: AtomicU64,
    irq_top: UnsafeCell<Option<(&'static core::panic::Location<'static>, u64, u64)>>,
    posted: AtomicU64,
    received: AtomicU64,
    received_at: AtomicU64,
    crossed: AtomicU64,
    kernel: AtomicU64,
    irq: AtomicU64,
}
// Only this Hart writes attribution with local interrupts masked. Stage scopes
// additionally retain their COMMIT admission. Reporting follows the Hart barrier.
unsafe impl Sync for Local {}
impl Local {
    /// # Safety
    /// `slot` uniquely names aligned, allocated storage for one uninitialized
    /// Local. No reader can access it until every field is initialized.
    unsafe fn initialize(slot: *mut Self) {
        // A Local is much larger than a boot stack's usable temporary space in
        // debug code. Construct each small field in its final allocation;
        // passing Local::new() by value can overflow before secondary startup.
        unsafe {
            let metrics = core::ptr::addr_of_mut!((*slot).metrics).cast::<Distribution>();
            for index in 0..METRICS {
                metrics.add(index).write(Distribution::new());
            }
            let phases = core::ptr::addr_of_mut!((*slot).phases).cast::<Distribution>();
            for index in 0..3 * METRICS {
                phases.add(index).write(Distribution::new());
            }
            core::ptr::addr_of_mut!((*slot).slowest)
                .write(UnsafeCell::new(transaction::Slowest::new()));
            for phase in 0..3 {
                core::ptr::addr_of_mut!((*slot).phase_slowest)
                    .cast::<UnsafeCell<transaction::Slowest>>()
                    .add(phase)
                    .write(UnsafeCell::new(transaction::Slowest::new()));
                core::ptr::addr_of_mut!((*slot).phase_ready)
                    .cast::<UnsafeCell<Option<(usize, execution::Sample)>>>()
                    .add(phase)
                    .write(UnsafeCell::new(None));
                core::ptr::addr_of_mut!((*slot).phase_irq).cast::<UnsafeCell<Option<(&'static core::panic::Location<'static>, u64, u64)>>>().add(phase).write(UnsafeCell::new(None));
            }
            core::ptr::addr_of_mut!((*slot).transaction).write(AtomicU64::new(0));
            core::ptr::addr_of_mut!((*slot).parts)
                .write(UnsafeCell::new(transaction::Parts::new()));
            core::ptr::addr_of_mut!((*slot).ready).write(UnsafeCell::new(None));
            core::ptr::addr_of_mut!((*slot).irq_site).write(UnsafeCell::new(None));
            core::ptr::addr_of_mut!((*slot).irq_transaction).write(AtomicU64::new(0));
            core::ptr::addr_of_mut!((*slot).irq_top).write(UnsafeCell::new(None));
            core::ptr::addr_of_mut!((*slot).posted).write(AtomicU64::new(0));
            core::ptr::addr_of_mut!((*slot).received).write(AtomicU64::new(0));
            core::ptr::addr_of_mut!((*slot).received_at).write(AtomicU64::new(0));
            core::ptr::addr_of_mut!((*slot).crossed).write(AtomicU64::new(0));
            core::ptr::addr_of_mut!((*slot).kernel).write(AtomicU64::new(0));
            core::ptr::addr_of_mut!((*slot).irq).write(AtomicU64::new(0));
        }
    }
}
static LOCALS: OnceLock<Box<[Local]>> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
// Ordinary boot uses the whole running lifetime. A test composition can
// bracket its warmed workload through these same private diagnostic transitions.
static TIMELINE: timeline::Timeline = timeline::Timeline::new();
pub(crate) fn running() {
    TIMELINE.open(stamp());
}
pub(crate) fn stopping() {
    TIMELINE.close(stamp());
}
pub(crate) fn init() -> Result<(), crate::basis::nest::NestError> {
    let mut storage = Box::<[Local]>::try_new_uninit_slice(hart::hart_count())
        .map_err(|_| crate::basis::nest::NestError::OutOfMemory)?;
    for slot in storage.iter_mut() {
        // SAFETY: each Box slot is disjoint and remains unpublished until the
        // completed array is installed in LOCALS below.
        unsafe { Local::initialize(slot.as_mut_ptr()) };
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
pub(crate) fn record(metric: Metric, ticks: u64, end: u64) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        local.metrics[metric as usize].record(ticks);
        if let Some(phase) = TIMELINE.phase(end.saturating_sub(ticks), end) {
            local.phases[phase][metric as usize].record(ticks);
        } else {
            local.crossed.fetch_add(1, Ordering::Relaxed);
        }
    }
}
/// This generation describes a published scheduling notification attempt,
/// not a firmware-delivery proof. Exhaustion yields unknown rather than reuse.
pub(crate) fn notification(destination: usize) -> (u64, u64) {
    let Some(local) = LOCALS.get().and_then(|all| all.get(destination)) else {
        return (0, 0);
    };
    let generation = local
        .posted
        .try_update(Ordering::AcqRel, Ordering::Acquire, |old| {
            old.checked_add(1)
        })
        .ok()
        .and_then(|old| old.checked_add(1))
        .unwrap_or(0);
    (generation, stamp())
}
pub(crate) fn ipi_received() {
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        local
            .received
            .store(local.posted.load(Ordering::Acquire), Ordering::Relaxed);
        local.received_at.store(stamp(), Ordering::Release);
    }
}
pub(crate) fn receipt() -> (u64, u64) {
    LOCALS
        .get()
        .and_then(|all| all.get(hart::hart_id().get()))
        .map(|local| {
            (
                local.received.load(Ordering::Relaxed),
                local.received_at.load(Ordering::Relaxed),
            )
        })
        .unwrap_or((0, 0))
}
pub(crate) fn ready(task: usize, sample: execution::Sample, end: u64) {
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        // Trap return still masks local interrupts. The scheduler cannot move
        // this running Task while the continuation owns its context.
        let top = unsafe { &mut *local.ready.get() };
        if top
            .as_ref()
            .is_none_or(|(_, old)| sample.total_ticks > old.total_ticks)
        {
            *top = Some((task, sample));
        }
        if let Some(phase) = TIMELINE.phase(end.saturating_sub(sample.total_ticks), end) {
            let top = unsafe { &mut *local.phase_ready[phase].get() };
            if top
                .as_ref()
                .is_none_or(|(_, old)| sample.total_ticks > old.total_ticks)
            {
                *top = Some((task, sample));
            }
        }
    }
}
pub(crate) fn elapsed(metric: Metric, start: u64) {
    if start != 0 {
        let end = clock::now().as_ticks();
        let ticks = end.saturating_sub(start);
        record(metric, ticks, end);
        if matches!(metric, Metric::SpinWait) {
            account(Work::SpinWait, ticks, None);
        }
        if matches!(metric, Metric::ControllerClaim) {
            account(Work::ControllerClaim, ticks, None);
        }
    }
}
#[track_caller]
pub(crate) fn enter() {
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        local.kernel.store(stamp(), Ordering::Relaxed);
        local.irq.store(stamp(), Ordering::Relaxed);
        unsafe { *local.irq_site.get() = Some(core::panic::Location::caller()) };
        local
            .irq_transaction
            .store(local.transaction.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}
pub(crate) fn leave() {
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        let start = local.kernel.swap(0, Ordering::Relaxed);
        elapsed(Metric::Kernel, start);
        let _ = pause_irq();
    }
}
pub(crate) fn pause_irq() -> bool {
    let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) else {
        return false;
    };
    let start = local.irq.swap(0, Ordering::Relaxed);
    if start != 0 {
        let end = stamp();
        let ticks = end.saturating_sub(start);
        record(Metric::IrqOff, ticks, end);
        if let Some(site) = unsafe { *local.irq_site.get() } {
            let top = unsafe { &mut *local.irq_top.get() };
            if top.as_ref().is_none_or(|(_, old, _)| ticks > *old) {
                *top = Some((site, ticks, local.irq_transaction.load(Ordering::Relaxed)));
            }
            if let Some(phase) = TIMELINE.phase(start, end) {
                let top = unsafe { &mut *local.phase_irq[phase].get() };
                if top.as_ref().is_none_or(|(_, old, _)| ticks > *old) {
                    *top = Some((site, ticks, local.irq_transaction.load(Ordering::Relaxed)));
                }
            }
        }
    }
    start != 0
}
#[track_caller]
pub(crate) fn resume_irq(measured: bool) {
    if measured && let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        local.irq.store(stamp(), Ordering::Relaxed);
        unsafe { *local.irq_site.get() = Some(core::panic::Location::caller()) };
        local
            .irq_transaction
            .store(local.transaction.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}
pub(crate) fn transaction(
    start: u64,
    site: &'static core::panic::Location<'static>,
) -> Transaction {
    let now = stamp();
    let wait = now.saturating_sub(start);
    if start != 0 {
        record(Metric::OuterWait, wait, now);
    }
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        // The newly admitted outer owner has interrupts masked. No sampling
        // scope may migrate or outlive this exact admission.
        unsafe { *local.parts.get() = transaction::Parts::new() };
        local.transaction.store(now, Ordering::Relaxed);
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
    let end = stamp();
    let hold = end.saturating_sub(transaction.start);
    record(Metric::OuterHold, hold, end);
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        local.transaction.store(0, Ordering::Relaxed);
        // Called only by the last COMMIT guard, before releasing ownership.
        let sample = transaction::Sample {
            site: transaction.site,
            hold,
            wait: transaction.wait,
            transaction: transaction.start,
            parts: unsafe { *local.parts.get() },
        };
        unsafe { &mut *local.slowest.get() }.record(sample);
        if let Some(phase) = TIMELINE.phase(transaction.start, end) {
            unsafe { &mut *local.phase_slowest[phase].get() }.record(sample);
        }
    }
}

/// Only used inside an existing COMMIT admission. This guard adds no locking,
/// allocation or I/O and cannot be sent to a different Hart.
pub(crate) struct Scope<'a> {
    _admission: &'a crate::basis::CommitGuard,
    work: Work,
    start: u64,
    transaction: u64,
    _local: core::marker::PhantomData<*const ()>,
}
pub(crate) fn scope(work: Work, admission: &crate::basis::CommitGuard) -> Scope<'_> {
    #[cfg(debug_assertions)]
    crate::basis::assert_held();
    let transaction = LOCALS
        .get()
        .and_then(|all| all.get(hart::hart_id().get()))
        .map_or(0, |local| local.transaction.load(Ordering::Relaxed));
    Scope {
        _admission: admission,
        work,
        start: stamp(),
        transaction,
        _local: core::marker::PhantomData,
    }
}
fn account(work: Work, ticks: u64, expected: Option<u64>) {
    if let Some(local) = LOCALS.get().and_then(|all| all.get(hart::hart_id().get())) {
        let current = local.transaction.load(Ordering::Relaxed);
        if current != 0 && expected.is_none_or(|id| id == current) {
            // All callers execute with local interrupts masked. The reception
            // window's assembly callback neither reads nor changes this state.
            unsafe { &mut *local.parts.get() }.record(work, ticks);
        }
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if self.start != 0 && self.transaction != 0 {
            account(
                self.work,
                stamp().saturating_sub(self.start),
                Some(self.transaction),
            );
        }
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
    crate::putln!(
        "latency-storage: harts={} bytes-per-hart={} outer-sites=8-per-phase+8-all ready-max=1-per-phase+1-all irq-max=1-per-phase+1-all",
        all.len(),
        core::mem::size_of::<Local>()
    );
    let (opened, closed) = TIMELINE.bounds();
    crate::putln!(
        "latency-window: opened={} closed={} boundary=whole-interval",
        opened,
        closed
    );
    for (hart, local) in all.iter().enumerate() {
        crate::putln!(
            "latency-crossed: hart={} count={}",
            hart,
            local.crossed.load(Ordering::Relaxed)
        );
        if let Some((site, ticks, transaction)) = unsafe { &*local.irq_top.get() } {
            crate::putln!(
                "latency-irq: hart={} ticks={} start-transaction={} origin={}:{}:{} cause=unknown-host-firmware-or-kernel",
                hart,
                ticks,
                transaction,
                site.file(),
                site.line(),
                site.column()
            );
        }
        if let Some((task, sample)) = unsafe { &*local.ready.get() } {
            crate::putln!(
                "latency-ready: hart={} task={} sample={:?} reasons=1:ineligible,2:budget,4:outranked notice=attempt,receipt-covers-posted-generation unknown=host-pause,firmware,coalescing,uncovered-notice",
                hart,
                task,
                sample
            );
        }
        for (phase, label) in ["boot", "running", "teardown"].iter().enumerate() {
            for sample in unsafe { &*local.phase_slowest[phase].get() }
                .samples
                .iter()
                .flatten()
            {
                crate::putln!(
                    "latency-phase-outer: hart={} phase={} transaction={} hold={} wait={} site={}:{} ticks={:?} calls={:?} order={} inclusive=1",
                    hart,
                    label,
                    sample.transaction,
                    sample.hold,
                    sample.wait,
                    sample.site.file(),
                    sample.site.line(),
                    sample.parts.ticks,
                    sample.parts.calls,
                    transaction::ORDER
                );
            }
            if let Some((task, sample)) = unsafe { &*local.phase_ready[phase].get() } {
                crate::putln!(
                    "latency-phase-ready: hart={} phase={} task={} sample={:?}",
                    hart,
                    label,
                    task,
                    sample
                );
            }
            if let Some((site, ticks, transaction)) = unsafe { &*local.phase_irq[phase].get() } {
                crate::putln!(
                    "latency-phase-irq: hart={} phase={} ticks={} start-transaction={} origin={}:{} cause=unknown-host-firmware-or-kernel",
                    hart,
                    label,
                    ticks,
                    transaction,
                    site.file(),
                    site.line()
                );
            }
        }
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
            crate::putln!(
                "latency-outer-parts: hart={} transaction={} site={}:{} ticks={:?} calls={:?} order={} inclusive=1",
                hart,
                sample.transaction,
                sample.site.file(),
                sample.site.line(),
                sample.parts.ticks,
                sample.parts.calls,
                transaction::ORDER
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
        for (phase, metrics) in ["boot", "running", "teardown"].iter().zip(&local.phases) {
            for (name, metric) in NAMES.iter().zip(metrics) {
                let (count, max, bins) = metric.snapshot();
                if count != 0 {
                    crate::putln!(
                        "latency-phase: hart={} phase={} metric={} count={} max={} bins={:?} boundary=whole-interval",
                        hart,
                        phase,
                        name,
                        count,
                        max,
                        bins
                    );
                }
            }
        }
    }
}
