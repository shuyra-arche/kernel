use crate::basis::{
    room::{deadline, shutdown},
    sign,
    unit::lifecycle::tally,
};
use crate::metal::interrupt::{self, ipi};
const BEACON_TICK: u64 = 250_000;
// Idle registration and the wait loop share their wakeup protocol.

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::metal::hart;
use crate::metal::hart::HartId;

/// 每颗 hart 一位：置着 = 它正睡在空闲环里。
static WAITING: [AtomicUsize; WAITING_WORDS] = [const { AtomicUsize::new(0) }; WAITING_WORDS];

const WAITING_WORDS: usize = crate::metal::layout::MAX_HART_SLOTS / usize::BITS as usize;

pub(crate) fn enter(hart: HartId) {
    debug_assert!(
        hart.get() < crate::metal::layout::MAX_HART_SLOTS,
        "sleep hart {hart} beyond MAX_HART_SLOTS"
    );
    let (word, bit) = hart.bit();
    WAITING[word].fetch_or(bit, Ordering::AcqRel);
}

pub(crate) fn leave(hart: HartId) {
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
pub(super) fn pick(cursor: usize, compatible: impl Fn(HartId) -> bool) -> HartId {
    let n = hart::hart_count();
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

pub(super) fn fetch() -> usize {
    #[cfg(debug_assertions)]
    {
        crate::basis::assert_unlocked();
        crate::metal::switcher::stack::assert_active();
    }
    interrupt::enable_external();
    if let Some(pa) = super::dispatch(true) {
        return pa;
    }
    wait()
}

fn wait() -> usize {
    let me = hart::hart_id();
    enter(me);
    let found = super::dispatch(true);
    if let Some(pa) = found {
        leave(me);
        return pa;
    }
    if tally::done() {
        shutdown::finish();
    }
    loop {
        // Clear first, then inspect work and deadlines. An IPI racing this
        // clear is either observed through the queue or stays pending for WFI.
        ipi::acknowledge();
        crate::basis::sign::endpoint::delivery::advance();
        let redeemed = deadline::elapsed() && sign::redeem();
        // No runnable Task may remain to reenter scheduler::run. Advance the
        // retained husks here too, outside COMMIT and before rearming/WFI.
        if !super::isolated(me) {
            crate::basis::unit::lifecycle::bury();
        }
        if tally::done() {
            leave(me);
            shutdown::finish();
        }
        shutdown::observe(me);

        let external_pending =
            !super::isolated(me) && crate::basis::nest::authority::external::maintain();
        let fallback = if shutdown::active() || external_pending {
            crate::metal::clock::now()
                .as_ticks()
                .saturating_add(BEACON_TICK)
        } else {
            crate::metal::generic::clock::DISARMED
        };
        deadline::arm(
            fallback.min(super::slot::current().deadline(crate::metal::clock::now().as_ticks())),
        );
        interrupt::enable_external();
        if interrupt::external_pending()
            && !crate::basis::room::interrupt::external().is_some_and(|route| route.handle())
        {
            interrupt::disable_external();
        }
        // Let notified remote Harts receive the Tasks just placed there
        // before the deadline owner tries to steal them back.
        if let Some(pa) = super::dispatch(!redeemed) {
            leave(me);
            return pa;
        }
        #[cfg(feature = "latency")]
        crate::metal::measure::leave();
        hart::wait();
        #[cfg(feature = "latency")]
        crate::metal::measure::enter();
    }
}
