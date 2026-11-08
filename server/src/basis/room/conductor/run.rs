//! 本核的运行环：trap 返回之后，这一步决定接下来跑谁。

use crate::basis::room::deadline;
use crate::basis::room::halt;
use crate::basis::room::halt::beacon;
use crate::basis::room::scheduler;
use crate::basis::sign;
use crate::basis::unit::tally;
use crate::machine::hart;
use crate::machine::stop::hush;
use riscv::register::sie;
use riscv::register::sip;

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

/// 没在停机路上时可以一觉睡到天亮的时限。
const WFI_FAR: u64 = 1 << 60;
/// 停机路上改按这个时限回来看一眼。
const BEACON_TICK: u64 = 250_000;

fn dispatch() -> Option<usize> {
    // Ready tasks must not escape every scheduler container between commits.
    let _commit = crate::basis::commit();
    let s = current();
    s.pull()
        .or_else(scheduler::steal)
        .and_then(|task| s.seat(task))
}

pub(super) fn fetch() -> usize {
    #[cfg(debug_assertions)]
    {
        crate::basis::assert_unlocked();
        let sp: usize;
        unsafe { core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack)) };
        let me = hart::hart_id();
        let base = crate::machine::switcher::stack::trap_stack_base(me).get();
        let edge = crate::machine::switcher::stack::trap_stack_edge(me).get();
        assert!(
            sp > base && sp <= edge,
            "idle must use the guarded hart stack"
        );
    }
    loop {
        unsafe {
            sie::set_sext();
        }
        if let Some(pa) = dispatch() {
            return pa;
        }
        if tally::done() {
            halt::halt();
        }
        if let Some(pa) = wait() {
            return pa;
        }
    }
}

fn wait() -> Option<usize> {
    let me = hart::hart_id();
    scheduler::idle::sleep(me);
    let found = dispatch();
    if let Some(pa) = found {
        scheduler::idle::wake(me);
        return Some(pa);
    }
    if tally::done() {
        halt::halt();
    }
    loop {
        if tally::done() {
            unsafe { sip::clear_ssoft() };
            scheduler::idle::wake(me);
            halt::halt();
        }
        beacon::idle(me);

        let device_pending = crate::basis::unit::space::external::maintain();
        let fallback = if beacon::shutting_down() || device_pending {
            BEACON_TICK
        } else {
            WFI_FAR
        };
        deadline::beat_until(fallback);
        unsafe {
            sie::set_sext();
        }
        if sip::read().sext() && !crate::machine::iommu::riscv::interrupt::handle() {
            let resources = crate::basis::room::interrupt::get();
            let result = crate::basis::sign::endpoint::trigger(&resources.supervisor_external);
            crate::basis::room::interrupt::note_external(result.is_err(), true);
        }
        if let Some(pa) = dispatch() {
            scheduler::idle::wake(me);
            return Some(pa);
        }
        unsafe {
            core::arch::asm!("wfi");
        }
        if sign::redeem() {
            break;
        }
        if let Some(pa) = dispatch() {
            scheduler::idle::wake(me);
            return Some(pa);
        }
        unsafe { sip::clear_ssoft() };
    }
    unsafe { sip::clear_ssoft() };
    scheduler::idle::wake(me);
    None
}
