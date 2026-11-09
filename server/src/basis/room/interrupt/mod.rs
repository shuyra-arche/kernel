//! One router owns all sources on the admitted controller: reserved queues and
//! driver notifications never independently overwrite its enable words.
use self::source::Source;
use crate::basis::sign::endpoint::Endpoint;
use crate::metal::sync::OnceLock;
use crate::metal::{
    generic::interrupt::{Delivery, Irq},
    hart,
    sync::SpinLock,
};
use alloc::sync::Arc;
use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::platform::riscv::devices::Plic as Plan;
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
    controller: Box<dyn Delivery>,
    physical: usize,
    limit: usize,
    hart: hart::HartId,
    queues: [Irq; 3],
    pending: AtomicUsize,
    counts: [AtomicUsize; 3],
    sources: SpinLock<Vec<Arc<Source>>>,
}
/// All hardware effects follow complete descriptor validation at boot.
pub(crate) fn install(controller: Box<dyn Delivery>, plan: Plan<'_>) -> Result<(), PendFail> {
    EXTERNAL
        .set(Router {
            controller,
            physical: plan.base,
            limit: plan.sources,
            hart: hart::hart_id(),
            queues: plan.queues.map(|line| Irq {
                controller: plan.base,
                line,
            }),
            pending: AtomicUsize::new(if plan.queues == [0; 3] { 0 } else { 7 }),
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
    route.controller.mask(irq);
    route.controller.prioritize(irq);
    sources.push(source);
    Ok(())
}
impl Router {
    pub(crate) fn handle(&self) -> bool {
        if hart::hart_id() != self.hart {
            return false;
        }
        #[cfg(feature = "latency")]
        let irq_entry = crate::metal::measure::stamp();
        let _commit = crate::basis::commit();
        for _ in 0..4 {
            let Some(irq) = self.controller.claim_masked() else {
                break;
            };
            note_external();
            if let Some(index) = self.queues.iter().position(|q| *q == irq) {
                self.finish(irq, false);
                self.counts[index].fetch_add(1, Ordering::Relaxed);
                self.pending.fetch_or(1 << index, Ordering::Release);
            } else {
                let endpoint = self.sources.lock().iter().find(|e| e.irq == irq).cloned();
                if let Some(endpoint) = endpoint {
                    if endpoint.deliver() {
                        #[cfg(feature = "latency")]
                        crate::metal::measure::elapsed(
                            crate::metal::measure::Metric::IrqSign,
                            irq_entry,
                        );
                    }
                } else {
                    self.finish(irq, false);
                }
                // Unknown, revoked or already-pending sources stay masked.
            }
        }
        true
    }
    pub(crate) fn finish(&self, irq: Irq, rearm: bool) {
        self.controller.unmask(irq);
        self.controller.complete(irq);
        if !rearm {
            self.controller.mask(irq);
        }
    }
    pub(crate) fn shut(&self, irq: Irq) {
        self.controller.mask(irq);
    }
    pub(crate) fn open(&self, irq: Irq) {
        self.controller.unmask(irq);
    }
    pub(crate) fn request(&self) {
        let _ = crate::metal::interrupt::ipi::send(self.hart);
    }
    pub(crate) fn rearm(&self) {
        if hart::hart_id() != self.hart {
            return;
        }
        let _commit = crate::basis::commit();
        let count = self.sources.lock().len();
        for at in 0..count {
            let source = self.sources.lock()[at].clone();
            source.maintain();
        }
        let pending = self.pending.swap(0, Ordering::AcqRel);
        for (index, irq) in self.queues.iter().enumerate() {
            if pending & (1 << index) != 0 {
                self.controller.unmask(*irq);
            }
        }
    }
    pub(crate) fn report(&self) {
        crate::putln!(
            "iommu-irqs: cq={} fq={} pq={}",
            self.counts[0].load(Ordering::Relaxed),
            self.counts[1].load(Ordering::Relaxed),
            self.counts[2].load(Ordering::Relaxed)
        );
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
            root.narrow(limits, None)?;
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
