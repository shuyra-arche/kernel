//! Compiled as a child of the real Flat owner in the isolated test composition.
//! Test-only hooks add this module, start its Kernel Task and time matching
//! SpinLock guards; production declarations/visibility remain unchanged.
use crate::basis::unit::{task::Task, team::kernel};
use crate::entry::boot::BootError;
use crate::metal::{clock, hart, layout::MAX_HART_SLOTS};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use kernel_api::{abi::operation::RegisterCall, address::VirtAddr};

#[cfg(feature = "flat-probe-contracts")]
mod contracts;
#[cfg(feature = "flat-probe-timing")]
mod timing;

pub(crate) fn start() -> Result<(), BootError> {
    #[cfg(feature = "flat-probe-contracts")]
    contracts::prepare()?;
    #[cfg(feature = "flat-probe-contracts")]
    let entry = contracts::run as *const () as usize;
    #[cfg(feature = "flat-probe-timing")]
    let entry = timing::run as *const () as usize;
    let task = kernel()
        .expect("kernel team")
        .task()
        .entry(VirtAddr::new(entry))
        .hold()
        .map_err(BootError::Bootstrap)?;
    crate::basis::room::shutdown::watch(&task);
    Task::release(&task).map_err(|source| BootError::BootstrapRelease {
        task: task.ident.id,
        source,
    })
}

fn now() -> u64 {
    clock::now().as_ticks()
}
fn note(args: core::fmt::Arguments<'_>) {
    use core::fmt::Write;
    // Kernel Tasks use a low-half supervisor stack. The ordinary buffered
    // console intentionally rejects low-half pointers, so pass bytes through
    // the existing DBCN byte backend; this never runs in a measured interval.
    let mut out = crate::metal::console::Emergency::new();
    core::fmt::write(&mut out, args).unwrap();
    out.write_str("\n").unwrap();
    out.finish();
}
fn yield_now() {
    use kernel_api::abi::call::room::operations::Yield;
    unsafe {
        core::arch::asm!("ebreak", in("a7") Yield::SLOT,
            lateout("a0") _, lateout("a1") _, lateout("a2") _, options(nostack));
    }
}
fn exit() -> ! {
    use kernel_api::abi::call::unit::operations::Exit;
    unsafe {
        core::arch::asm!("ebreak", in("a0") 0usize, in("a1") 0usize,
            in("a2") 0usize, in("a3") 0usize, in("a4") 0usize,
            in("a7") Exit::SLOT, options(noreturn));
    }
}

// The only interval with scheduling deliberately suppressed is a test barrier
// (to force cross-Hart overlap) or allocator exhaustion (to keep setup stable).
// It is never held across yield/exit. Window polls retain interrupt reception.
#[cfg(feature = "flat-probe-contracts")]
struct Pin(bool);
#[cfg(feature = "flat-probe-contracts")]
impl Pin {
    fn new() -> Self {
        let enabled = riscv::register::sstatus::read().sie();
        unsafe { riscv::register::sstatus::clear_sie() };
        Self(enabled)
    }
}
#[cfg(feature = "flat-probe-contracts")]
impl Drop for Pin {
    fn drop(&mut self) {
        if self.0 {
            unsafe { riscv::register::sstatus::set_sie() };
        }
    }
}

static LOCK: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Metric {
    count: AtomicU64,
    max: AtomicU64,
    sum: AtomicU64,
    bins: [AtomicU64; 32],
}
impl Metric {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            max: AtomicU64::new(0),
            sum: AtomicU64::new(0),
            bins: [const { AtomicU64::new(0) }; 32],
        }
    }
    fn record(&self, ticks: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.max.fetch_max(ticks, Ordering::Relaxed);
        self.sum.fetch_add(ticks, Ordering::Relaxed);
        self.bins[(64 - ticks.leading_zeros() as usize).min(31)].fetch_add(1, Ordering::Relaxed);
    }
}
static HOLDS: [Metric; MAX_HART_SLOTS] = [const { Metric::new() }; MAX_HART_SLOTS];
#[cfg(feature = "flat-probe-timing")]
static CALLS: Metric = Metric::new();

pub(crate) fn lock_enter(address: usize) -> u64 {
    if ACTIVE.load(Ordering::Relaxed) && LOCK.load(Ordering::Relaxed) == address {
        now().max(1)
    } else {
        0
    }
}
pub(crate) fn lock_leave(start: u64) {
    if start != 0 {
        HOLDS[hart::hart_id().get()].record(now().saturating_sub(start));
    }
}
