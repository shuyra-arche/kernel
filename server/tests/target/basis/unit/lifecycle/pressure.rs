//! Target-only allocation refusal during resident wait enrollment. The scope
//! ends on the trap stack before scheduler handoff; no denial crosses migration.
use core::alloc::{Allocator, Layout};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static ACTIVE: [AtomicBool; crate::metal::layout::MAX_HART_SLOTS] =
    [const { AtomicBool::new(false) }; crate::metal::layout::MAX_HART_SLOTS];
static CHECKS: AtomicUsize = AtomicUsize::new(0);
static DENIED: AtomicUsize = AtomicUsize::new(0);
static BLOCKED: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Denial(crate::metal::hart::HartId);

pub(crate) fn enter() -> Denial {
    let hart = crate::metal::hart::hart_id();
    assert!(!ACTIVE[hart.get()].swap(true, Ordering::Relaxed));
    let guard = Denial(hart);
    let allocation = crate::metal::allocator::portal::PORTAL_ALLOCATOR
        .allocate(Layout::from_size_align(64, 8).unwrap());
    assert!(allocation.is_err(), "allocation refusal was not active");
    CHECKS.fetch_add(1, Ordering::Relaxed);
    guard
}

/// Called by the composed allocator before its normal allocation guard. Every
/// attempt is counted, so a hidden fallible allocation cannot pass silently.
pub(crate) fn denied() -> bool {
    let active = ACTIVE[crate::metal::hart::hart_id().get()].load(Ordering::Relaxed);
    if active {
        DENIED.fetch_add(1, Ordering::Relaxed);
    }
    active
}

impl Drop for Denial {
    fn drop(&mut self) {
        assert!(ACTIVE[self.0.get()].swap(false, Ordering::Relaxed));
    }
}

pub(crate) fn blocked() {
    BLOCKED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn report() {
    let checks = CHECKS.load(Ordering::Relaxed);
    let denied = DENIED.load(Ordering::Relaxed);
    let blocked = BLOCKED.load(Ordering::Relaxed);
    assert!(checks > 0, "resident wait was not exercised");
    assert!(blocked > 0, "resident never enrolled a blocking wait");
    assert_eq!(checks, denied, "resident enrollment attempted allocation");
    assert!(ACTIVE.iter().all(|active| !active.load(Ordering::Relaxed)));
    crate::putln!(
        "PASS lifecycle-pressure checks={checks} denied={denied} blocked={blocked} live=0"
    );
}
