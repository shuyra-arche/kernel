//! PLIC owns only delivery. All queue evidence is read through the retained
//! Engine; neither a claim nor completion of an IRQ settles a memory obligation.
pub(crate) mod plan;
use crate::machine::hart;
use crate::machine::sync::OnceLock;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use plan::Plan;
struct Route {
    base: usize,
    physical: usize,
    size: usize,
    context: usize,
    hart: hart::HartId,
    queues: [u32; 3],
}
static ROUTE: OnceLock<Route> = OnceLock::new();
static CQ: AtomicUsize = AtomicUsize::new(0);
static FQ: AtomicUsize = AtomicUsize::new(0);
static PQ: AtomicUsize = AtomicUsize::new(0);
static MASKED: AtomicUsize = AtomicUsize::new(0);
fn order() {
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) };
}
unsafe fn read(base: usize, offset: usize) -> u32 {
    order();
    unsafe { ((base + offset) as *const u32).read_volatile() }
}
unsafe fn write(base: usize, offset: usize, value: u32) {
    order();
    unsafe { ((base + offset) as *mut u32).write_volatile(value) };
    order();
}
/// # Safety
/// Plan was validated against trusted QEMU DTB; base maps that whole PLIC and
/// its aliases are withheld. Firmware owns M contexts; Kernel owns S contexts.
pub(crate) unsafe fn install(base: usize, plan: Plan) -> Result<(), ()> {
    if ROUTE.get().is_some() {
        return Err(());
    }
    for context in &plan.contexts {
        for word in 0..=plan.sources / 32 {
            unsafe { write(base, 0x2000 + context * 0x80 + word * 4, 0) };
        }
        unsafe { write(base, 0x200000 + context * 0x1000, 0) };
    }
    for irq in plan.queues {
        unsafe { write(base, irq as usize * 4, 1) };
    }
    ROUTE
        .set(Route {
            base,
            physical: plan.base,
            size: plan.size,
            context: plan.selected,
            hart: hart::hart_id(),
            queues: plan.queues,
        })
        .map_err(|_| ())?;
    MASKED.store(7, Ordering::Release);
    rearm();
    Ok(())
}
/// Mask before complete; bounded bottom-half work drains/observes queues before
/// rearming. A permanently faulty queue cannot spin inside the interrupt trap.
pub(crate) fn handle() -> bool {
    let Some(route) = ROUTE.get() else {
        return false;
    };
    if hart::hart_id() != route.hart {
        return false;
    }
    let claim = 0x200004 + route.context * 0x1000;
    for _ in 0..4 {
        let irq = unsafe { read(route.base, claim) };
        if irq == 0 {
            break;
        }
        let enable = 0x2000 + route.context * 0x80 + (irq as usize / 32) * 4;
        unsafe {
            write(
                route.base,
                enable,
                read(route.base, enable) & !(1 << (irq % 32)),
            )
        };
        if let Some(index) = route.queues.iter().position(|v| *v == irq) {
            [&CQ, &FQ, &PQ][index].fetch_add(1, Ordering::Relaxed);
            MASKED.fetch_or(1 << index, Ordering::Release);
        }
        unsafe { write(route.base, claim, irq) };
    }
    true
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
        let enable = 0x2000 + route.context * 0x80 + (*irq as usize / 32) * 4;
        unsafe {
            write(
                route.base,
                enable,
                read(route.base, enable) | (1 << (*irq % 32)),
            )
        };
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
