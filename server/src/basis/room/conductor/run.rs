//! 本核的运行环：trap 返回之后，这一步决定接下来跑谁。

use crate::basis::room::deadline;
use crate::basis::room::halt;
use crate::basis::room::halt::beacon;
use crate::basis::room::scheduler;
use crate::basis::sign;
use crate::basis::unit::lifecycle::tally;
use crate::metal::hart;
use crate::metal::interrupt::{self, ipi};
use crate::metal::stop::hush;

use super::current;

/// trap 返回时的下一步：返回下一个任务的帧地址。
pub(crate) fn run() -> usize {
    hush();
    crate::basis::unit::lifecycle::bury();
    let commit = crate::basis::commit();
    if current()
        .running_task()
        .is_some_and(|task| crate::basis::unit::lifecycle::take_doomed(task.ident.id).is_some())
    {
        drop(commit);
        return crate::basis::unit::lifecycle::quit();
    }
    let next = current().advance();
    drop(commit);
    match next {
        Some(pa) => pa,
        None => fetch(),
    }
}

// 本核的空闲环：先在自己槽里找，找不到就偷、就睡，醒来再看。

/// 停机路上改按这个时限回来看一眼。
const BEACON_TICK: u64 = 250_000;

fn dispatch(allow_steal: bool) -> Option<usize> {
    // Ready tasks must not escape every scheduler container between commits.
    let _commit = crate::basis::commit();
    let s = current();
    let task = s.pull().or_else(|| {
        if allow_steal {
            scheduler::steal()
        } else {
            None
        }
    });
    task.and_then(|task| s.seat(task))
}

pub(super) fn fetch() -> usize {
    #[cfg(debug_assertions)]
    {
        crate::basis::assert_unlocked();
        crate::metal::switcher::stack::assert_active();
    }
    interrupt::enable_external();
    if let Some(pa) = dispatch(true) {
        return pa;
    }
    wait()
}

fn wait() -> usize {
    let me = hart::hart_id();
    scheduler::idle::sleep(me);
    let found = dispatch(true);
    if let Some(pa) = found {
        scheduler::idle::wake(me);
        return pa;
    }
    if tally::done() {
        halt::halt();
    }
    loop {
        if tally::done() {
            ipi::acknowledge();
            scheduler::idle::wake(me);
            halt::halt();
        }
        // Clear first, then inspect work and deadlines. An IPI racing this
        // clear is either observed through the queue or stays pending for WFI.
        ipi::acknowledge();
        let redeemed = deadline::elapsed() && sign::redeem();
        beacon::idle(me);

        let external_pending = crate::basis::nest::authority::external::maintain();
        let fallback = if beacon::shutting_down() || external_pending {
            crate::metal::clock::now()
                .as_ticks()
                .saturating_add(BEACON_TICK)
        } else {
            crate::metal::generic::clock::DISARMED
        };
        deadline::arm(fallback);
        interrupt::enable_external();
        if interrupt::external_pending()
            && !crate::basis::room::interrupt::external().is_some_and(|route| route.handle())
        {
            crate::basis::room::interrupt::deliver_external(true);
        }
        // Let notified remote Harts receive the Tasks just placed there
        // before the deadline owner tries to steal them back.
        if let Some(pa) = dispatch(!redeemed) {
            scheduler::idle::wake(me);
            return pa;
        }
        hart::wait();
    }
}
