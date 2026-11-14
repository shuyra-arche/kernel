use super::super::{Allocation, Flat};
use super::*;
use crate::basis::cell::page::Owner;
use kernel_api::model::cell::CellLevel;

pub(super) extern "C" fn run() -> ! {
    let level = CellLevel::new(1).unwrap();
    for _ in 0..64 {
        let flat = Flat::acquire(Owner::Kernel, Allocation::at_level(512, level).unwrap()).unwrap();
        LOCK.store(&flat.state as *const _ as usize, Ordering::Relaxed);
        ACTIVE.store(true, Ordering::Relaxed);
        let start = now();
        let pages = flat.whole().materialize().unwrap();
        let ticks = now().saturating_sub(start);
        ACTIVE.store(false, Ordering::Relaxed);
        CALLS.record(ticks);
        drop(pages);
        drop(flat);
        yield_now();
    }
    for (index, metric) in HOLDS[..hart::hart_count()].iter().enumerate() {
        note(format_args!(
            "flat-hold: hart={} count={} max={} sum={} bins={:?}",
            index,
            metric.count.load(Ordering::Relaxed),
            metric.max.load(Ordering::Relaxed),
            metric.sum.load(Ordering::Relaxed),
            metric.bins.each_ref().map(|v| v.load(Ordering::Relaxed))
        ));
    }
    note(format_args!(
        "flat-call: count={} max={} sum={} bins={:?}",
        CALLS.count.load(Ordering::Relaxed),
        CALLS.max.load(Ordering::Relaxed),
        CALLS.sum.load(Ordering::Relaxed),
        CALLS.bins.each_ref().map(|v| v.load(Ordering::Relaxed))
    ));
    note(format_args!(
        "PASS flat-timing rounds=64 level=1 grain=2097152 ticks-hz={} proof=observation",
        crate::entry::boot::facts::info().hart.hertz()
    ));
    exit()
}
