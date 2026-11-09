use crate::entry::boot::facts;
mod local;

use crate::metal::generic::clock::Compare;

#[cfg(debug_assertions)]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::metal::{hart, layout::MAX_HART_SLOTS, sync::OnceLock};

use super::counter::Backend;
use local::LocalTimer;

// Each slot is initialized and accessed by its own Hart with interrupts masked.
static TIMERS: [OnceLock<LocalTimer<Backend>>; MAX_HART_SLOTS] =
    [const { OnceLock::new() }; MAX_HART_SLOTS];
static SUPERVISOR_STIMECMP: AtomicBool = AtomicBool::new(false);

pub(super) fn configure(supervisor_stimecmp: bool) {
    SUPERVISOR_STIMECMP.store(supervisor_stimecmp, Ordering::Release);
}

pub(crate) fn initialize() {
    let slot = hart::hart_id();
    // The ISA bit alone does not authorize access through menvcfg/mcounteren.
    let direct = SUPERVISOR_STIMECMP.load(Ordering::Acquire) && facts::info().hart.sstc(slot);
    let backend = if direct { Backend::Sstc } else { Backend::Sbi };
    let timer = LocalTimer::new(backend).expect("disarm local timer during Hart initialization");
    assert!(
        TIMERS[slot.get()].set(timer).is_ok(),
        "local timer already initialized"
    );
}

fn local() -> &'static LocalTimer<Backend> {
    TIMERS[hart::hart_id().get()]
        .get()
        .expect("local timer not initialized")
}

pub(crate) fn set_timer(at: u64) {
    if !local()
        .set(Compare::from_ticks(at))
        .expect("program local timer")
    {
        #[cfg(debug_assertions)]
        SKIPPED.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn retry(now: u64, at: u64) {
    local().retry(now, at).expect("program local retry");
}

// An unchanged, elapsed comparator remains pending until reprogrammed.
pub(crate) fn elapsed() -> bool {
    let setting = local().setting().expect("local timer state unknown");
    setting != Compare::OFF && super::now().as_ticks() >= setting.ticks()
}

#[cfg(debug_assertions)]
static SKIPPED: AtomicU64 = AtomicU64::new(0);

#[cfg(debug_assertions)]
pub(crate) fn measures() -> ([(u64, u64); 2], u64) {
    (super::counter::measures(), SKIPPED.load(Ordering::Relaxed))
}
