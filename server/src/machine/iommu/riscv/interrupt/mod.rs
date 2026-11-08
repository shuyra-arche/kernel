//! PLIC owns only delivery. All queue evidence is read through the retained
//! Engine; neither a claim nor completion of an IRQ settles a memory obligation.
pub(crate) mod plan;
use crate::machine::hart;
use crate::machine::interrupt::{Claim, Controller, Irq, plic::Plic};
use crate::machine::sync::OnceLock;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use plan::Plan;
struct Route<C> {
    controller: C,
    physical: usize,
    size: usize,
    hart: hart::HartId,
    queues: [Irq; 3],
}
static ROUTE: OnceLock<Route<Plic>> = OnceLock::new();
static CQ: AtomicUsize = AtomicUsize::new(0);
static FQ: AtomicUsize = AtomicUsize::new(0);
static PQ: AtomicUsize = AtomicUsize::new(0);
static MASKED: AtomicUsize = AtomicUsize::new(0);
/// # Safety
/// Plan was validated against trusted QEMU DTB; base maps that whole PLIC and
/// its aliases are withheld. Firmware owns M contexts; Kernel owns S contexts.
pub(crate) unsafe fn install(base: usize, plan: Plan) -> Result<(), ()> {
    if ROUTE.get().is_some() {
        return Err(());
    }
    let controller = unsafe {
        Plic::install(
            base,
            plan.base,
            &plan.contexts,
            plan.selected,
            plan.sources,
            plan.queues,
        )
    };
    let queues = plan.queues.map(|line| controller.irq(line));
    ROUTE
        .set(Route {
            controller,
            physical: plan.base,
            size: plan.size,
            hart: hart::hart_id(),
            queues,
        })
        .map_err(|_| ())?;
    MASKED.store(7, Ordering::Release);
    rearm();
    Ok(())
}
/// Finish the claim and mask the line. Bounded bottom-half work drains or
/// observes queues before rearming it.
pub(crate) fn handle() -> bool {
    let Some(route) = ROUTE.get() else {
        return false;
    };
    if hart::hart_id() != route.hart {
        return false;
    }
    route.handle();
    true
}

impl<C: Controller> Route<C> {
    fn handle(&self) {
        for _ in 0..4 {
            let Some(claim) = self.controller.claim() else {
                break;
            };
            let claimed = claim.finish();
            if let Some(index) = self.queues.iter().position(|irq| *irq == claimed) {
                [&CQ, &FQ, &PQ][index].fetch_add(1, Ordering::Relaxed);
                MASKED.fetch_or(1 << index, Ordering::Release);
            }
        }
    }
}

pub(crate) fn rearm() {
    let Some(route) = ROUTE.get() else {
        return;
    };
    // Only the routed hart changes enable words; other harts can progress data.
    if hart::hart_id() != route.hart {
        return;
    }
    let masked = MASKED.swap(0, Ordering::AcqRel);
    for (index, irq) in route.queues.iter().enumerate() {
        if masked & (1 << index) == 0 {
            continue;
        }
        route.controller.unmask(*irq);
    }
}
pub(crate) fn report() {
    if ROUTE.get().is_some() {
        crate::machine::console::_write(format_args!(
            "iommu-irqs: cq={} fq={} pq={}\n",
            CQ.load(Ordering::Relaxed),
            FQ.load(Ordering::Relaxed),
            PQ.load(Ordering::Relaxed)
        ));
    }
}

pub(crate) fn overlaps(base: usize, end: usize) -> bool {
    ROUTE
        .get()
        .is_some_and(|route| base < route.physical + route.size && route.physical < end)
}
