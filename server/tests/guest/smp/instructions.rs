//! Independently withhold instruction acknowledgements on real Harts.
use super::*;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*};
static ARRIVED: AtomicUsize = AtomicUsize::new(0);
static PHASE: AtomicUsize = AtomicUsize::new(0);
static STALE: AtomicBool = AtomicBool::new(false);

fn until(mut ready: impl FnMut() -> bool) {
    let deadline = crate::metal::clock::now().as_ticks()
        + crate::entry::boot::facts::info().hart.hertz() as u64 * 10;
    while !ready() {
        assert!(
            crate::metal::clock::now().as_ticks() < deadline,
            "probe progress"
        );
        core::hint::spin_loop();
    }
}

pub(crate) fn run() {
    let me = hart::hart_id().get();
    ARRIVED.fetch_add(1, AcqRel);
    until(|| ARRIVED.load(Acquire) == hart::hart_count());
    if me == 0 {
        let page = Arc::new(
            crate::basis::cell::allocate(crate::basis::cell::page::Owner::Kernel).unwrap(),
        );
        let mut retention = page.prepare_hardware(false).unwrap();
        retention.publish();
        let retention = retention.retire();
        if hart::hart_count() > 1 {
            PHASE.store(1, Release);
            assert_eq!(synchronize_instructions(), Err(Error::TimedOut));
            assert!(!page.allows_return());
            PHASE.store(2, Release);
            assert_eq!(synchronize_instructions(), Err(Error::TimedOut));
            assert!(STALE.load(Acquire));
            assert!(!page.allows_return());
        }
        PHASE.store(3, Release);
        synchronize_instructions().unwrap();
        // SAFETY: no user mapping was installed by this bookkeeping probe;
        // the real backend nevertheless completed every participating CPU.
        unsafe { retention.settle() };
        assert!(page.allows_return());
        crate::putln!(
            "PASS smp-instructions harts={} bounded=1 retained=1 retry=1",
            hart::hart_count()
        );
        PHASE.store(4, Release);
    } else if me == 1 {
        let mailbox = &INSTRUCTIONS[me];
        until(|| PHASE.load(Acquire) >= 1 && mailbox.pending().is_some());
        let old = mailbox.pending().unwrap();
        // Withhold one real Hart through the first timeout and the next round.
        until(|| PHASE.load(Acquire) >= 2 && mailbox.pending().is_some_and(|r| r != old));
        unsafe {
            core::arch::asm!(
                "fence rw, rw",
                "sfence.vma",
                "fence rw, rw",
                "fence.i",
                options(nostack)
            );
            mailbox.complete(old);
            mailbox.complete(old); // Delayed duplicate cannot satisfy round two.
        }
        STALE.store(true, Release);
        until(|| PHASE.load(Acquire) >= 3);
        until(|| {
            service();
            PHASE.load(Acquire) == 4
        });
    } else {
        until(|| {
            service();
            PHASE.load(Acquire) == 4
        });
    }
}
