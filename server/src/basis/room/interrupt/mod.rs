//! One router owns all sources on the admitted controller: reserved queues and
//! driver notifications never independently overwrite its enable words.
mod progress;
use self::source::Source;
use crate::basis::sign::endpoint::Endpoint;
use crate::metal::sync::OnceLock;
use crate::metal::sync::TrapGuard;
use crate::metal::{
    generic::interrupt::{Delivery, Irq},
    hart,
    sync::SpinLock,
};
use alloc::sync::Arc;
use alloc::{boxed::Box, vec::Vec};
#[cfg(feature = "latency")]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::platform::riscv::devices::Plic as Plan;
use progress::{BATCH, Claim, Progress};
static CLAIMED: AtomicUsize = AtomicUsize::new(0);

fn note_external() {
    CLAIMED.fetch_add(1, Ordering::Relaxed);
}
pub(crate) fn claimed() -> usize {
    CLAIMED.load(Ordering::Relaxed)
}

pub(crate) mod source;
static EXTERNAL: OnceLock<Router> = OnceLock::new();
pub(crate) fn external() -> Option<&'static Router> {
    EXTERNAL.get()
}

/// Registration is preflighted while the recipient ledger and COMMIT are held.
pub(crate) fn dispatch(
    task: &Arc<crate::basis::unit::task::Task>,
    entry: &Arc<crate::basis::pend::PendEntry>,
) -> Result<(), PendFail> {
    if let Some(sign) = entry.target().sign()
        && let Some(source) = external().and_then(|r| r.source(&sign))
    {
        source.attach(task, entry)?;
    }
    Ok(())
}
pub(crate) fn authorize(
    sign: &crate::basis::sign::endpoint::Endpoint,
    task: &crate::basis::unit::task::Task,
    token: kernel_api::model::identity::PendId,
) -> Result<(), kernel_api::abi::call::sign::SignFail> {
    if let Some(source) = external().and_then(|r| r.source(sign)) {
        source.authorize(task, token)?;
    }
    Ok(())
}
pub(crate) fn withdraw(token: kernel_api::model::identity::PendId) {
    if let Some(route) = external() {
        let _commit = crate::basis::commit();
        route.withdraw(token);
    }
}
pub(crate) fn region(sign: &crate::basis::sign::endpoint::Endpoint) -> Option<usize> {
    external()?.source(sign).map(|s| s.region)
}
pub(crate) fn identifies(
    sign: &crate::basis::sign::endpoint::Endpoint,
    controller: u64,
    line: u32,
) -> bool {
    external()
        .and_then(|r| r.source(sign))
        .is_some_and(|s| s.irq.controller as u64 == controller && s.irq.line == line)
}

pub(crate) fn consumed(endpoint: &crate::basis::sign::endpoint::Endpoint) {
    if let Some(router) = external()
        && router.source(endpoint).is_some()
    {
        router.request();
    }
}

pub(crate) struct Router {
    controller: SpinLock<Box<dyn Delivery>>,
    progress: SpinLock<Progress>,
    serving: AtomicBool,
    requested: AtomicBool,
    physical: usize,
    limit: usize,
    hart: hart::HartId,
    queues: [Irq; 3],
    pending: AtomicUsize,
    deferred: AtomicBool,
    deferrals: AtomicUsize,
    #[cfg(feature = "latency")]
    first_entry: AtomicU64,
    counts: [AtomicUsize; 3],
    sources: SpinLock<Vec<Arc<Source>>>,
}
/// All hardware effects follow complete descriptor validation at boot.
pub(crate) fn install(controller: Box<dyn Delivery>, plan: Plan<'_>) -> Result<(), PendFail> {
    EXTERNAL
        .set(Router {
            controller: SpinLock::new(controller),
            progress: SpinLock::new(Progress::new()),
            serving: AtomicBool::new(false),
            requested: AtomicBool::new(false),
            physical: plan.base,
            limit: plan.sources,
            hart: hart::hart_id(),
            queues: plan.queues.map(|line| Irq {
                controller: plan.base,
                line,
            }),
            pending: AtomicUsize::new(if plan.queues == [0; 3] { 0 } else { 7 }),
            deferred: AtomicBool::new(true),
            deferrals: AtomicUsize::new(0),
            #[cfg(feature = "latency")]
            first_entry: AtomicU64::new(0),
            counts: core::array::from_fn(|_| AtomicUsize::new(0)),
            sources: SpinLock::new(Vec::new()),
        })
        .map_err(|_| PendFail::Denied)?;
    EXTERNAL.get().ok_or(PendFail::Denied)?.rearm();
    Ok(())
}
pub(crate) fn register(controller: usize, line: u32, region: usize) -> Result<(), PendFail> {
    let route = EXTERNAL.get().ok_or(PendFail::Denied)?;
    let irq = Irq { controller, line };
    let _commit = crate::basis::commit();
    if controller != route.physical
        || line == 0
        || line as usize > route.limit
        || route.queues.contains(&irq)
    {
        return Err(PendFail::Denied);
    }
    let mut sources = route.sources.lock();
    if sources.len() >= 64 || sources.iter().any(|e| e.irq == irq) {
        return Err(PendFail::Denied);
    }
    sources.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
    let source = Arc::try_new(Source::new(irq, region)?).map_err(|_| PendFail::OutOfMemory)?;
    let controller = route.controller.lock();
    controller.mask(irq);
    controller.prioritize(irq);
    drop(controller);
    sources.push(source);
    Ok(())
}
impl Router {
    /// Local exclusion prevents reentry; the hardware gate serializes enable
    /// words against peer-Hart revocation. It is never retained into COMMIT.
    fn service(&self) -> Option<Service<'_>> {
        if hart::hart_id() != self.hart {
            return None;
        }
        let trap = unsafe { TrapGuard::save() };
        if self
            .serving
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        Some(Service {
            router: self,
            _trap: trap,
        })
    }
    fn defer(&self) {
        self.deferred.store(true, Ordering::Release);
        self.deferrals.fetch_add(1, Ordering::Relaxed);
        crate::metal::interrupt::disable_external();
        super::deadline::retry();
    }
    fn claim(&self) {
        #[cfg(debug_assertions)]
        crate::basis::assert_unlocked();
        #[cfg(feature = "latency")]
        let entered = {
            let first = self.first_entry.swap(0, Ordering::Relaxed);
            if first == 0 {
                crate::metal::measure::stamp()
            } else {
                first
            }
        };
        for _ in 0..BATCH {
            if !self.progress.lock().vacant() {
                break;
            }
            #[cfg(feature = "latency")]
            let start = crate::metal::measure::stamp();
            let irq = self.controller.lock().claim_masked();
            #[cfg(feature = "latency")]
            crate::metal::measure::elapsed(crate::metal::measure::Metric::ControllerClaim, start);
            let Some(irq) = irq else {
                break;
            };
            note_external();
            self.progress.lock().retain(Claim {
                irq,
                #[cfg(feature = "latency")]
                entered,
            });
        }
    }
    /// Called with one COMMIT admission. Each source accepts responsibility
    /// before its record leaves the router; unknown/reserved claims leave as
    /// owned completion work and are completed only after releasing COMMIT.
    fn publish(&self, _commit: &crate::basis::CommitGuard) -> [Option<Claim>; BATCH] {
        #[cfg(feature = "latency")]
        let admitted = crate::metal::measure::stamp();
        let mut complete = [const { None }; BATCH];
        let claims = self.progress.lock().take();
        for (at, claim) in claims.into_iter().enumerate() {
            let Some(claim) = claim else {
                continue;
            };
            if let Some(index) = self.queues.iter().position(|q| *q == claim.irq) {
                self.counts[index].fetch_add(1, Ordering::Relaxed);
                self.pending.fetch_or(1 << index, Ordering::Release);
                complete[at] = Some(claim);
                continue;
            }
            let source = self
                .sources
                .lock()
                .iter()
                .find(|e| e.irq == claim.irq)
                .cloned();
            if let Some(source) = source {
                if source.deliver() {
                    #[cfg(feature = "latency")]
                    {
                        use crate::metal::measure::{Metric, record, stamp};
                        let published = stamp();
                        record(
                            Metric::IrqAdmit,
                            admitted.saturating_sub(claim.entered),
                            admitted,
                        );
                        record(
                            Metric::IrqPublish,
                            published.saturating_sub(admitted),
                            published,
                        );
                        record(
                            Metric::IrqSign,
                            published.saturating_sub(claim.entered),
                            published,
                        );
                    }
                }
                // Source now owns the masked claim, including no live receiver
                // or a retained pending Sign. No completion is inferred here.
                self.requested.store(true, Ordering::Release);
            } else {
                complete[at] = Some(claim);
            }
        }
        complete
    }
    fn complete(&self, batch: [Option<Claim>; BATCH]) {
        for claim in batch.into_iter().flatten() {
            self.finish(claim.irq, false);
        }
    }
    pub(crate) fn handle(&self) -> bool {
        if hart::hart_id() != self.hart {
            return false;
        }
        #[cfg(feature = "latency")]
        {
            let _ = self.first_entry.compare_exchange(
                0,
                crate::metal::measure::stamp(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
        let Some(_service) = self.service() else {
            self.defer();
            return true;
        };
        self.claim();
        let Some(commit) = crate::basis::try_commit() else {
            self.defer();
            return true;
        };
        let complete = self.publish(&commit);
        drop(commit);
        self.complete(complete);
        self.deferred.store(true, Ordering::Release);
        super::deadline::retry();
        true
    }
    pub(crate) fn finish(&self, irq: Irq, rearm: bool) {
        let controller = self.controller.lock();
        controller.unmask(irq);
        controller.complete(irq);
        if !rearm {
            controller.mask(irq);
        }
    }
    pub(crate) fn shut(&self, irq: Irq) {
        self.controller.lock().mask(irq);
    }
    pub(crate) fn open(&self, irq: Irq) {
        self.controller.lock().unmask(irq);
    }
    pub(crate) fn request(&self) {
        self.requested.store(true, Ordering::Release);
        self.deferred.store(true, Ordering::Release);
        let _ = crate::metal::interrupt::ipi::send(self.hart);
    }
    /// Safe trap-return/idle boundary, outside scheduler ownership. FIFO COMMIT
    /// admission here prevents a stream of peer Yield calls starving a claim.
    pub(crate) fn advance(&self) {
        self.rearm();
    }
    pub(crate) fn rearm(&self) {
        if hart::hart_id() != self.hart || !self.deferred.load(Ordering::Acquire) {
            return;
        }
        let Some(_service) = self.service() else {
            return;
        };
        #[cfg(debug_assertions)]
        crate::basis::assert_unlocked();
        self.deferred.store(false, Ordering::Release);
        // A prior failed admission retained the actual claims. A canceled CPU
        // level is also rechecked, settling its original attempt timestamp.
        self.claim();
        let commit = crate::basis::commit();
        let complete = self.publish(&commit);
        drop(commit);
        self.complete(complete);
        let count = self.sources.lock().len();
        if self.requested.swap(false, Ordering::AcqRel) {
            self.progress.lock().refresh(count);
        }
        for _ in 0..BATCH {
            let Some(at) = self.progress.lock().next(count) else {
                break;
            };
            let source = self.sources.lock()[at].clone();
            source.maintain();
        }
        let pending = self.pending.swap(0, Ordering::AcqRel);
        for (index, irq) in self.queues.iter().enumerate() {
            if pending & (1 << index) != 0 {
                self.controller.lock().unmask(*irq);
            }
        }
        if self.progress.lock().pending() || self.requested.load(Ordering::Acquire) {
            self.deferred.store(true, Ordering::Release);
            super::deadline::retry();
        }
        crate::metal::interrupt::enable_external();
    }
    pub(crate) fn report(&self) {
        crate::putln!(
            "iommu-irqs: cq={} fq={} pq={}",
            self.counts[0].load(Ordering::Relaxed),
            self.counts[1].load(Ordering::Relaxed),
            self.counts[2].load(Ordering::Relaxed)
        );
        crate::putln!("irq-deferred: {}", self.deferrals.load(Ordering::Relaxed));
    }
    pub(crate) fn grants(
        &self,
        registry: &mut crate::entry::boot::resources::Registry,
    ) -> Result<(), PendFail> {
        use crate::basis::pend::{PendEntry, Target};
        use kernel_api::{
            boot::ledger::Name,
            model::{identity::Mark, modifiable::Modifiable, pend::Operations, sign::SignRights},
        };
        let count = self.sources.lock().len();
        for at in 0..count {
            let source = self.sources.lock()[at].clone();
            let endpoint = source.endpoint.clone();
            let irq = source.irq;
            let view = registry.region(source.region).ok_or(PendFail::Denied)?;
            source.associate(view.target().share_view().ok_or(PendFail::Denied)?)?;
            let root = PendEntry::try_new(
                Target::Sign(endpoint.clone()),
                Mark::NONE,
                Modifiable::No,
                None,
            )?;
            let mut limits = root.authority().limits();
            limits.operations = Operations::Sign(SignRights::LISTEN | SignRights::CONSUME);
            root.narrow(limits)?;
            registry
                .register(
                    Name::Interrupt {
                        controller: irq.controller as u64,
                        source: irq.line,
                    },
                    root,
                )
                .map_err(|e| e.into_parts().0)?;
        }
        Ok(())
    }
    fn source(&self, endpoint: &Endpoint) -> Option<Arc<Source>> {
        self.sources
            .lock()
            .iter()
            .find(|s| s.endpoint.id == endpoint.id)
            .cloned()
    }
    fn withdraw(&self, token: kernel_api::model::identity::PendId) {
        let count = self.sources.lock().len();
        for at in 0..count {
            let source = self.sources.lock()[at].clone();
            source.withdraw(token);
        }
    }
}

/// The controller Hart cannot recursively claim while a batch is being handed
/// to software or completed. No guard survives a scheduling/context boundary.
struct Service<'a> {
    router: &'a Router,
    _trap: TrapGuard,
}
impl Drop for Service<'_> {
    fn drop(&mut self) {
        self.router.serving.store(false, Ordering::Release);
    }
}
