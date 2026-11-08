//! 空着的核：谁睡着了记在位图上，活往哪放、怎么把核叫醒。

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::metal::hart;
use crate::metal::hart::HartId;

/// 每颗 hart 一位：置着 = 它正睡在空闲环里。
static WAITING: [AtomicUsize; WAITING_WORDS] = [const { AtomicUsize::new(0) }; WAITING_WORDS];

/// 挑核时转着圈从不同的位起，免得总挑到同一颗。
static PICK_CURSOR: AtomicUsize = AtomicUsize::new(0);

const WAITING_WORDS: usize = crate::metal::layout::MAX_HART_SLOTS / usize::BITS as usize;

pub(crate) fn sleep(hart: HartId) {
    debug_assert!(
        hart.get() < crate::metal::layout::MAX_HART_SLOTS,
        "sleep hart {hart} beyond MAX_HART_SLOTS"
    );
    let (word, bit) = hart.bit();
    WAITING[word].fetch_or(bit, Ordering::AcqRel);
}

pub(crate) fn wake(hart: HartId) {
    debug_assert!(
        hart.get() < crate::metal::layout::MAX_HART_SLOTS,
        "wake hart {hart} beyond MAX_HART_SLOTS"
    );
    let (word, bit) = hart.bit();
    WAITING[word].fetch_and(!bit, Ordering::AcqRel);
}

pub(super) fn waiting(hart: HartId) -> bool {
    debug_assert!(
        hart.get() < crate::metal::layout::MAX_HART_SLOTS,
        "waiting hart {hart} beyond MAX_HART_SLOTS"
    );
    let (word, bit) = hart.bit();
    WAITING[word].load(Ordering::Acquire) & bit != 0
}

/// Rotate within eligible Harts, preferring idle destinations. A sparse
/// capability set must not inherit the gaps or bias of the physical bitmap.
pub(super) fn pick(compatible: impl Fn(HartId) -> bool, prefer_remote: bool) -> HartId {
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
    if prefer_remote {
        let remote = || eligible().filter(|candidate| *candidate != me);
        let count = remote().count();
        if count != 0 {
            return remote().nth(cursor % count).unwrap();
        }
    }
    let count = eligible().count();
    assert!(count != 0, "Task has no compatible Hart");
    eligible().nth(cursor % count).unwrap()
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
    let _ = crate::metal::interrupt::ipi::send(hart);
}
