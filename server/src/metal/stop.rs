use crate::metal::hart;
use ::core::sync::atomic::AtomicUsize;
use ::core::sync::atomic::Ordering;
use sbi;
use sbi::fid;

static ALARMER: AtomicUsize = AtomicUsize::new(usize::MAX);
static QUIET: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn hush() {
    let owner = ALARMER.load(Ordering::Acquire);
    if owner != usize::MAX && owner != hart::hart_id().get() {
        hunker();
    }
}

fn hunker() -> ! {
    crate::metal::mmu::identity::vacate();
    unsafe { ::core::arch::asm!("csrci sstatus, 2") };
    // This path never returns: each peer acknowledges once after vacating its lease.
    QUIET.fetch_add(1, Ordering::Release);
    let _ = sbi::HsmCall::new(fid::Hsm::Stop).call();
    loop {
        ::core::hint::spin_loop();
    }
}

pub(crate) fn claim() -> bool {
    let me = hart::hart_id().get();
    match ALARMER.compare_exchange(usize::MAX, me, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => true,
        Err(owner) if owner == me => false,
        Err(_) => hunker(),
    }
}

pub(crate) fn quiesced_peers() -> usize {
    QUIET.load(Ordering::Acquire)
}

pub(crate) fn alarm() -> bool {
    let won = claim();
    if won {
        broadcast();
    }
    won
}

pub(crate) fn broadcast() {
    let me = hart::hart_id();
    for slot in (0..hart::hart_count()).map(hart::HartId::new) {
        if slot == me {
            continue;
        }
        let _ = crate::metal::interrupt::ipi::send(slot);
    }
}

pub(crate) fn reset() {
    let _ = sbi::SystemResetCall::new(fid::SystemReset::SystemReset).call();
}
