//! Interrupt reception while a kernel continuation cannot enter the ordinary
//! trampoline. The temporary vector uses only declared scratch registers: no
//! stack switch, Rust call, allocation, lock, controller claim or Task change.
//! External delivery is attempted only after the last exclusion guard leaves;
//! timer/IPI levels remain owned by the ordinary scheduling entry.
use crate::metal::{hart, layout::MAX_HART_SLOTS, sync::OnceLock};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[repr(align(64))]
struct Local {
    ready: AtomicBool,
    depth: AtomicUsize,
    serving: AtomicBool,
    pending: AtomicUsize,
    #[cfg(feature = "latency")]
    opened: AtomicUsize,
    #[cfg(feature = "latency")]
    captured: AtomicUsize,
}
impl Local {
    const fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            depth: AtomicUsize::new(0),
            serving: AtomicBool::new(false),
            pending: AtomicUsize::new(0),
            #[cfg(feature = "latency")]
            opened: AtomicUsize::new(0),
            #[cfg(feature = "latency")]
            captured: AtomicUsize::new(0),
        }
    }
}
static LOCALS: [Local; MAX_HART_SLOTS] = [const { Local::new() }; MAX_HART_SLOTS];
static SERVICE: OnceLock<fn(usize)> = OnceLock::new();
fn local() -> &'static Local {
    &LOCALS[hart::hart_id().get()]
}
pub(crate) fn bind(service: fn(usize)) {
    assert!(SERVICE.set(service).is_ok());
}
pub(crate) fn arm() {
    local().ready.store(true, Ordering::Release);
}
pub(crate) fn enter() {
    local().depth.fetch_add(1, Ordering::Relaxed);
}
pub(crate) fn leave() {
    assert!(local().depth.fetch_sub(1, Ordering::Relaxed) != 0);
    drain();
}
pub(crate) fn drain() {
    let local = local();
    if local.depth.load(Ordering::Relaxed) != 0
        || !local.ready.load(Ordering::Acquire)
        || local.pending.load(Ordering::Relaxed) == 0
    {
        return;
    }
    let Some(service) = SERVICE.get() else { return };
    if local.serving.swap(true, Ordering::Relaxed) {
        return;
    }
    let pending = local.pending.swap(0, Ordering::AcqRel);
    service(pending);
    local.serving.store(false, Ordering::Relaxed);
}

core::arch::global_asm!(
    ".pushsection .text",
    ".balign 4",
    "__kernel_irq_window:",
    "csrr a0, sip",
    "and a0, a0, t5",
    "amoor.d a1, a0, (a3)",
    // One reception per window. The caller restores the exact enabled mask;
    // level state is never claimed/cleared by this front half.
    "csrw sie, zero",
    "sret",
    ".popsection",
);
unsafe extern "C" {
    fn __kernel_irq_window();
}

/// Safe even with nested exclusion guards: only the leaf assembly vector may
/// execute. Saved trap CSRs and the original mask are restored before returning.
#[inline(never)]
pub(crate) fn poll() {
    let local = local();
    if !local.ready.load(Ordering::Acquire) {
        return;
    }
    #[cfg(feature = "latency")]
    local.opened.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "latency")]
    let measured = crate::metal::measure::pause_irq();
    unsafe {
        core::arch::asm!(
            "csrr t0, sstatus",
            "csrci sstatus, 2",
            "csrr t1, stvec",
            "csrr t2, sepc",
            "csrr t3, scause",
            "csrr t4, stval",
            "csrr t5, sie",
            "csrw stvec, {vector}",
            "csrsi sstatus, 2",
            "nop",
            "csrci sstatus, 2",
            "csrw stvec, t1",
            "csrw sepc, t2",
            "csrw scause, t3",
            "csrw stval, t4",
            "csrw sie, t5",
            "csrw sstatus, t0",
            vector = in(reg) __kernel_irq_window as *const () as usize,
            in("a3") &local.pending as *const AtomicUsize,
            out("a0") _, out("a1") _,
            out("t0") _, out("t1") _, out("t2") _, out("t3") _, out("t4") _, out("t5") _,
            options(nostack),
        );
    }
    #[cfg(feature = "latency")]
    crate::metal::measure::resume_irq(measured);
    #[cfg(feature = "latency")]
    if local.pending.load(Ordering::Relaxed) != 0 {
        local.captured.fetch_add(1, Ordering::Relaxed);
    }
}
#[cfg(feature = "latency")]
pub(crate) fn stats() -> (usize, usize) {
    LOCALS[..hart::hart_count()]
        .iter()
        .fold((0, 0), |(opened, captured), local| {
            (
                opened + local.opened.load(Ordering::Relaxed),
                captured + local.captured.load(Ordering::Relaxed),
            )
        })
}
