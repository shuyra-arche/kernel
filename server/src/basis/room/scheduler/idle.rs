//! 空着的核：谁睡着了记在位图上，活往哪放、怎么把核叫醒。

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use sbi;
use sbi::ecall::SArgs;
use sbi::fid;

use crate::machine::hart;
use crate::machine::hart::HartId;

/// 每颗 hart 一位：置着 = 它正睡在空闲环里。
static WAITING: [AtomicUsize; WAITING_WORDS] = [const { AtomicUsize::new(0) }; WAITING_WORDS];

/// 挑核时转着圈从不同的位起，免得总挑到同一颗。
static PICK_CURSOR: AtomicUsize = AtomicUsize::new(0);

const WAITING_WORDS: usize = crate::machine::layout::MAX_HART_SLOTS / usize::BITS as usize;

pub(crate) fn sleep(hart: HartId) {
    debug_assert!(
        hart.get() < crate::machine::layout::MAX_HART_SLOTS,
        "sleep hart {hart} beyond MAX_HART_SLOTS"
    );
    let (word, bit) = hart.bit();
    WAITING[word].fetch_or(bit, Ordering::AcqRel);
}

pub(crate) fn wake(hart: HartId) {
    debug_assert!(
        hart.get() < crate::machine::layout::MAX_HART_SLOTS,
        "wake hart {hart} beyond MAX_HART_SLOTS"
    );
    let (word, bit) = hart.bit();
    WAITING[word].fetch_and(!bit, Ordering::AcqRel);
}

pub(super) fn waiting(hart: HartId) -> bool {
    debug_assert!(
        hart.get() < crate::machine::layout::MAX_HART_SLOTS,
        "waiting hart {hart} beyond MAX_HART_SLOTS"
    );
    let (word, bit) = hart.bit();
    WAITING[word].load(Ordering::Acquire) & bit != 0
}

/// Rotate within eligible Harts, preferring idle destinations. A sparse
/// capability set must not inherit the gaps or bias of the physical bitmap.
pub(super) fn pick(compatible: impl Fn(HartId) -> bool) -> HartId {
    let cursor = PICK_CURSOR.fetch_add(1, Ordering::Relaxed);
    let n = hart::hart_count();
    let me = hart::hart_id();
    let eligible = || {
        (0..n)
            .map(HartId::new)
            .filter(|candidate| compatible(*candidate))
    };
    let idle_count = eligible().filter(|candidate| waiting(*candidate)).count();
    if idle_count != 0 {
        if let Some(hart) = eligible()
            .filter(|candidate| waiting(*candidate))
            .nth(cursor % idle_count)
        {
            return hart;
        }
        // Idle flags can change between passes; compatibility is stable while
        // the Task is off-Hart. Continue with the complete eligible set.
    }
    let others = eligible().filter(|candidate| *candidate != me).count();
    if others != 0 {
        return eligible()
            .filter(|candidate| *candidate != me)
            .nth(cursor % others)
            .unwrap();
    }
    eligible().next().expect("Task has no compatible Hart")
}

/// 把所有睡着的核都叫一遍（停机之前用）。
pub(crate) fn yell() {
    for slot in (0..hart::hart_count()).map(HartId::new) {
        if waiting(slot) {
            nudge(slot);
        }
    }
}

/// 叫醒指定的那一核：它得重新看一眼自己手上的活。
pub(crate) fn nudge(hart: HartId) {
    let _ = try_nudge(hart);
}

pub(crate) fn acknowledge() {
    // Order the clear before the queue snapshot, including at the CSR/device
    // boundary. A producer that publishes after the snapshot leaves an IPI.
    unsafe { core::arch::asm!("csrci sip, 2", "fence o, rw", options(nostack)) };
}

pub(crate) fn try_nudge(hart: HartId) -> sbi::ecall::SResult<usize> {
    // Publish shared work before requesting an interrupt from the SEE.
    unsafe { core::arch::asm!("fence rw, ow", options(nostack)) };
    let (mask, base) = crate::machine::hart::firmware(hart).target();
    sbi::IpiCall::new(fid::Ipi::SendIpi)
        .args(SArgs {
            a0: mask,
            a1: base,
            ..Default::default()
        })
        .call()
}
