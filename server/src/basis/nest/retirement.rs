//! Deferred Page-completion discovery. Cell retains admission state, Nest owns
//! the weak source pass, and Sign owns each wait. No hardware receipt is inferred.
use super::index::source;
use crate::basis::cell::change;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) static WORK: crate::basis::sign::Resident = crate::basis::sign::Resident::new();
static LAST: AtomicU64 = AtomicU64::new(0);
static ADVANCING: AtomicBool = AtomicBool::new(false);

struct Turn;
impl Drop for Turn {
    fn drop(&mut self) {
        ADVANCING.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Progress {
    Idle,
    Ready,
    Blocked,
}

fn collect() {
    if change::completed() {
        source::request();
    }
}

/// One nonblocking turn: four weak gates and four source records. Interrupt
/// exclusion prevents cancellation between selecting a candidate and its Sign
/// effect. No guard, strong reference or cursor lease crosses the return.
pub(crate) fn advance() -> Progress {
    let _local = unsafe { crate::metal::sync::TrapGuard::save() };
    if crate::basis::room::scheduler::isolated(crate::metal::hart::hart_id()) {
        return Progress::Blocked;
    }
    if ADVANCING
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Progress::Blocked;
    }
    let _turn = Turn;
    collect();
    LAST.store(crate::metal::clock::now().as_ticks(), Ordering::Release);
    let gates = change::prune();
    let sources = source::advance();
    if gates || sources {
        Progress::Ready
    } else {
        Progress::Idle
    }
}

/// Exclusion-free scheduler/call boundary. Producer drops only set Cell's
/// retained completion bit; this boundary admits it to the worker's Sign Site.
/// Startup, shutdown and an unavailable/starved worker retain scheduler progress.
pub(crate) fn maintain() {
    if crate::basis::room::scheduler::isolated(crate::metal::hart::hart_id()) {
        // Do not run registry/Sign fan-out on a reserved Hart. Keep Cell's bit
        // and Nest's pass intact, including when the resident service died.
        WORK.notify();
        crate::basis::room::scheduler::nudge_ordinary();
        return;
    }
    collect();
    WORK.poll();
    let stopping = {
        let _commit = crate::basis::commit();
        crate::basis::room::shutdown::active()
    };
    if stopping {
        WORK.notify();
    }
    let pending = change::pending() || source::pending();
    let late = crate::metal::clock::now()
        .as_ticks()
        .saturating_sub(LAST.load(Ordering::Acquire))
        >= crate::metal::clock::duration_to_ticks(core::time::Duration::from_millis(100));
    if pending {
        WORK.notify();
        if !WORK.bound() || stopping || late {
            if advance() == Progress::Ready {
                // Fallback must not strand a partial pass on its way to WFI.
                crate::basis::room::deadline::retry();
            }
        }
    }
}

/// Once every Task is reaped, finish the remaining weak-discovery pass on a
/// scheduler stack. Dropping the last husk can itself request this pass, after
/// maintain() already ran. Do not make a departed worker finish its own shutdown.
pub(crate) fn settle() -> bool {
    WORK.poll();
    if advance() == Progress::Blocked {
        crate::basis::room::deadline::retry();
        return false;
    }
    collect();
    let settled = !change::pending() && !source::pending();
    if !settled {
        crate::basis::room::deadline::retry();
    }
    settled
}
