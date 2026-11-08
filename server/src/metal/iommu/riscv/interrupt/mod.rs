//! PLIC owns only delivery. All queue evidence is read through the retained
//! Engine; neither a claim nor completion of an IRQ settles a memory obligation.
pub(crate) mod plan;
use crate::metal::generic::interrupt::{Claim, Controller, External, Irq};
use crate::metal::hart;
use crate::metal::interrupt;
use crate::metal::interrupt::plic::Plic;
use alloc::boxed::Box;
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
static CQ: AtomicUsize = AtomicUsize::new(0);
static FQ: AtomicUsize = AtomicUsize::new(0);
static PQ: AtomicUsize = AtomicUsize::new(0);
static MASKED: AtomicUsize = AtomicUsize::new(0);
/// # Safety
/// Plan was validated against trusted QEMU DTB; base maps that whole PLIC and
/// its aliases are withheld. Firmware owns M contexts; Kernel owns S contexts.
pub(crate) unsafe fn install(base: usize, plan: Plan) -> Result<(), ()> {
    if interrupt::external().is_some() {
        return Err(());
    }
    let controller = unsafe { Plic::new(base, plan.base, plan.selected) };
    let queues = plan.queues.map(|line| controller.irq(line));
    let route = Box::try_new(Route {
        controller,
        physical: plan.base,
        size: plan.size,
        hart: hart::hart_id(),
        queues,
    })
    .map_err(|_| ())?;
    unsafe {
        route
            .controller
            .configure(&plan.contexts, plan.sources, plan.queues)
    };
    interrupt::install_external(route)?;
    MASKED.store(7, Ordering::Release);
    interrupt::external()
        .expect("installed IOMMU route")
        .rearm();
    Ok(())
}

impl<C: Controller + Send + Sync> External for Route<C> {
    /// Finish each claim and mask the line until bounded deferred work runs.
    fn handle(&self) -> bool {
        if hart::hart_id() != self.hart {
            return false;
        }
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
        true
    }

    fn rearm(&self) {
        // Only the routed Hart changes enable words.
        if hart::hart_id() != self.hart {
            return;
        }
        let masked = MASKED.swap(0, Ordering::AcqRel);
        for (index, irq) in self.queues.iter().enumerate() {
            if masked & (1 << index) != 0 {
                self.controller.unmask(*irq);
            }
        }
    }

    fn overlaps(&self, base: usize, end: usize) -> bool {
        base < self.physical + self.size && self.physical < end
    }
}

pub(crate) fn report() {
    if interrupt::external().is_some() {
        crate::metal::console::_write(format_args!(
            "iommu-irqs: cq={} fq={} pq={}\n",
            CQ.load(Ordering::Relaxed),
            FQ.load(Ordering::Relaxed),
            PQ.load(Ordering::Relaxed)
        ));
    }
}
