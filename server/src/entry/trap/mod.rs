mod fault;
pub(crate) mod resources;

use crate::basis::nest::FaultOutcome;
use crate::basis::nest::Nest;
use crate::basis::room::deadline;
use crate::basis::room::scheduler::run;
use crate::basis::room::scheduler::slot::identity::Identity;
use crate::basis::room::scheduler::slot::identity::ident;
use crate::basis::sign::redeem;
use crate::basis::unit::task::TaskIdent;
use crate::metal::generic::trap::TrapEvent;
use crate::metal::interrupt::{self, ipi};
use crate::metal::mmu::identity as asid;
use crate::metal::mmu::identity::Asid;
use crate::metal::switcher::context::TrapContext;
use crate::putln;
use alloc::sync::Arc;
use kernel_api::address::VirtAddr as KVirt;

use crate::metal::switcher::{stack::TRAP_STACK_CANARY, trap};

pub(crate) fn persist(frame: &TrapContext) -> bool {
    let Some(i) = ident() else {
        return false;
    };
    let Some(task) = i.live() else {
        return false;
    };
    if !task.team.nest.asid().is_kernel() {
        return false;
    }
    let Some(pa) = i.trap() else {
        return false;
    };
    let dst = pa.as_usize() as *mut TrapContext;
    unsafe {
        (*dst).copy_registers_from(frame);
    }
    true
}

pub(crate) fn handle(frame: &mut TrapContext, from_task: bool) -> *mut TrapContext {
    #[cfg(feature = "latency")]
    crate::metal::measure::enter();
    asid::occupy(Asid::kernel());

    if from_task {
        let task = crate::basis::room::scheduler::slot::current()
            .running_task()
            .expect("task trap without owner");
        assert_eq!(task.ident.frame_pa.as_usize(), frame as *const _ as usize);
        unsafe { task.capture_extensions(frame.status()) };
    }
    let ident = ident();

    crate::metal::stop::hush();

    let event = trap::capture();
    trap::check_stack();
    assert_eq!(
        frame.trap_stack_corrupt, TRAP_STACK_CANARY,
        "kernel trap frame corrupted"
    );
    #[cfg(debug_assertions)]
    if let Some(i) = ident.as_ref()
        && from_task
    {
        trap::check_task_stack(frame, i.task_id());
    }

    let next: *mut TrapContext = match event {
        TrapEvent::Timer => {
            if !deadline::elapsed() {
                // A comparator update can take time to clear a pending STIP.
                return resume(frame as *mut TrapContext);
            }
            crate::basis::nest::authority::external::maintain();
            interrupt::enable_external();
            redeem();
            if let Some(running) = ident.as_ref().and_then(Identity::live)
                && let Some(reason) = crate::basis::unit::lifecycle::take_doomed(running.id)
            {
                return quit(ident, reason);
            }
            drop(ident);
            if from_task {
                run() as *mut TrapContext
            } else if persist(frame) {
                run() as *mut TrapContext
            } else {
                deadline::arm(deadline::blind_deadline());
                frame as *mut TrapContext
            }
        }
        TrapEvent::Ipi => {
            ipi::acknowledge();
            if let Some(route) = crate::basis::room::interrupt::external() {
                route.rearm();
            }
            deadline::refresh();
            if let Some(running) = ident.as_ref().and_then(Identity::live)
                && let Some(reason) = crate::basis::unit::lifecycle::take_doomed(running.id)
            {
                return quit(ident, reason);
            }
            drop(ident);
            if from_task {
                run() as *mut TrapContext
            } else {
                frame as *mut TrapContext
            }
        }
        TrapEvent::External => {
            if crate::basis::room::interrupt::external().is_some_and(|route| route.handle()) {
                crate::basis::nest::authority::external::maintain();
            } else {
                interrupt::disable_external();
            }
            frame as *mut TrapContext
        }
        TrapEvent::Call => {
            if !from_task {
                crate::entry::report::halt::trap(frame, 3, 0);
            }
            match envcall(frame, ident) {
                Some(next) => next,
                None => crate::basis::unit::lifecycle::quit() as *mut TrapContext,
            }
        }
        TrapEvent::PageFault(fault) => {
            if !from_task {
                crate::entry::report::halt::trap(frame, fault.detail.cause, fault.detail.value);
            }
            // 先取定这一次错误的身份：真正终止任务前会先把 id 标量留下来。
            let (tid, nest) = {
                let running = ident
                    .as_ref()
                    .and_then(Identity::live)
                    .expect("user page fault without running task");
                (running.id.get(), running.team.nest.clone())
            };
            // 先纯探一次：不动页表就能判明该重试、该物化、该等还是不该续。
            let mut outcome = nest.probe_fault(fault.addr, fault.kind);
            if let FaultOutcome::Materialize = outcome {
                // 完整解析在提交边界之外：结失效、物化、发布，再重探一次。
                match nest.resolve_fault(fault.addr, fault.kind) {
                    Ok(next) => outcome = next,
                    // 真实资源失败：这是整份地址空间的失败，不是这一页自己失效；
                    // 照实记录，再放掉地址空间按原样终止。
                    Err(resource) => {
                        log::error!(
                            "page fault resource failure: {resource:?} at {:?} pc={:#x}",
                            fault.addr,
                            fault.detail.pc
                        );
                        drop(nest);
                        return fault::killed(ident, tid, fault);
                    }
                }
            }
            match outcome {
                FaultOutcome::Resolved => resume(frame as *mut TrapContext),
                // 一次完整解析之后仍要物化：并发只要求重探，与重试原指令同路，
                // 绝不与真实资源失败混同。
                FaultOutcome::Materialize => resume(frame as *mut TrapContext),
                // 承载不在（墓碑或在途）：按这段关系整段等一次，醒后重试原指令。
                FaultOutcome::Wait(mapping) => fault::wait(ident, fault, nest, mapping, frame),
                FaultOutcome::Invalid => {
                    drop(nest);
                    fault::killed(ident, tid, fault)
                }
            }
        }
        TrapEvent::IllegalInstruction(_) if from_task => {
            let task = crate::basis::room::scheduler::slot::current()
                .running_task()
                .expect("task fault without owner");
            let space = &task.ident.team.nest;
            let instruction = instruction(space, frame.sepc);
            let mut status = frame.status();
            let enabled = instruction.map(|word| task.enable_extension(word, &mut status));
            drop(task);
            match enabled {
                Some(Ok(true)) => {
                    frame.set_status(status);
                    frame as *mut TrapContext // Retry the same PC, including compressed instructions.
                }
                Some(Err(())) => {
                    log::error!("Task extended context allocation failed");
                    return kill_fault(ident);
                }
                _ => return kill_fault(ident),
            }
        }
        TrapEvent::Exception(fault) | TrapEvent::IllegalInstruction(fault) => {
            if from_task {
                let running = ident
                    .as_ref()
                    .and_then(Identity::live)
                    .expect("user exception without running task");
                let tid = running.id.get();
                putln!(
                    "user exception killed: tid={tid} cause={} stval={:#x}",
                    fault.cause,
                    fault.value
                );
                return kill_fault(ident);
            }
            crate::entry::report::halt::trap(frame, fault.cause, fault.value);
        }
    };

    trap::check_stack();
    resume(next)
}

pub(crate) fn resume(next: *mut TrapContext) -> *mut TrapContext {
    #[cfg(feature = "latency")]
    let mut ready = 0;
    let space = unsafe { (*next).address_space() };
    if !space.is_kernel() {
        let task = crate::basis::room::scheduler::slot::current()
            .running_task()
            .expect("return without Task owner");
        assert_eq!(task.ident.frame_pa.as_usize(), next as usize);
        let mut status = unsafe { (*next).status() };
        #[cfg(feature = "latency")]
        {
            ready = task
                .ready_stamp
                .swap(0, core::sync::atomic::Ordering::Relaxed);
        }
        unsafe {
            task.restore_extensions(&mut status);
            (*next).set_status(status);
        }
    }
    asid::occupy(space);
    #[cfg(feature = "latency")]
    {
        crate::metal::measure::elapsed(crate::metal::measure::Metric::ReadyRun, ready);
        crate::metal::measure::leave();
    }
    next
}

fn quit(ident: Option<Identity>, reason: usize) -> *mut TrapContext {
    crate::basis::unit::lifecycle::set_exit_reason(reason);
    drop(ident);
    resume(crate::basis::unit::lifecycle::quit() as *mut TrapContext)
}

fn kill_fault(ident: Option<Identity>) -> *mut TrapContext {
    let live = match &ident {
        Some(Identity::Live(live)) => Some(live),
        _ => None,
    };
    doom(
        live,
        kernel_api::model::unit::ExitCause::Fault,
        crate::basis::unit::lifecycle::EXIT_FAULT,
    );
    quit(ident, crate::basis::unit::lifecycle::EXIT_FAULT)
}

/// 环境调用那一支：先结清这张表上待结算的失效、再按真实长度把 `sepc` 推过那条
/// `ebreak`，最后才派发。
///
/// 三件“不知道”都不许往下走——失效结不掉、入口那个字节读不出来、推过之后
/// 越出地址宽度。那是这份地址空间的真实失败，与缺页那条同一种归责：照实记录，
/// 把这个任务**显式判死**（`Fault`），交回 `None` 让入口按原样把它换出去。
/// 绝不猜一个长度推进 `sepc`（错位会让客人跳过它自己写回答案的那一存），
/// 也绝不在没结清的表上做这一次调用；越界时连原 `sepc` 一并留在原地。
pub(crate) fn envcall(
    frame: &mut TrapContext,
    ident: Option<Identity>,
) -> Option<*mut TrapContext> {
    let Some(Identity::Live(ident)) = ident else {
        panic!("envcall without running task");
    };
    let space = &ident.team.nest;
    if let Err(resource) = space.retry_invalidation() {
        log::error!("envcall: the address space cannot settle its sync: {resource:?}");
        doom(
            Some(&ident),
            kernel_api::model::unit::ExitCause::Fault,
            crate::basis::unit::lifecycle::EXIT_FAULT,
        );
        return None;
    }
    let Some(length) = instr_len(space, frame.sepc) else {
        log::error!(
            "envcall: the instruction at sepc={:#x} is unreadable",
            frame.sepc.as_usize()
        );
        doom(
            Some(&ident),
            kernel_api::model::unit::ExitCause::Fault,
            crate::basis::unit::lifecycle::EXIT_FAULT,
        );
        return None;
    };
    let Some(next_pc) = frame.sepc.as_usize().checked_add(length) else {
        log::error!(
            "envcall: sepc={:#x} plus an instruction length overflows",
            frame.sepc.as_usize()
        );
        doom(
            Some(&ident),
            kernel_api::model::unit::ExitCause::Fault,
            crate::basis::unit::lifecycle::EXIT_FAULT,
        );
        return None;
    };
    frame.sepc = KVirt::wrap(next_pc);
    crate::entry::call::dispatch(frame, ident)
}

/// 环境调用那条指令的长度：`c.ebreak` 两字节、`ebreak` 四字节。
///
/// 首字节读不出来就是**不知道**，交 `None`，不凭空造一个长度。客人的正文只要
/// 在跑就读得出来（这条 `ebreak` 刚执行过）；读不出来是这份地址空间的失败。
fn instr_len(space: &Nest, sepc: KVirt) -> Option<usize> {
    let byte = space.instruction_byte(sepc)?;
    Some(if byte & 0b11 == 0b11 { 4 } else { 2 })
}

/// 把这个身份指到的任务显式判死并留下原因（换出由调用处负责）。
///
/// 身份指得到就只落在那一个上（环境调用与缺页两条路都按这份身份归责）；
/// 指不到时才落到此刻在跑的那一个。
fn doom(ident: Option<&Arc<TaskIdent>>, cause: kernel_api::model::unit::ExitCause, reason: usize) {
    let task = match ident {
        Some(ident) => {
            crate::basis::unit::task::registry::find(ident.id).and_then(|weak| weak.upgrade())
        }
        None => None,
    }
    .or_else(|| crate::basis::room::scheduler::slot::current().running_task());
    let Some(task) = task else { return };
    let _commit = crate::basis::commit();
    crate::basis::unit::team::exit::member_dying(&task, reason);
    if !task.dead() {
        task.state
            .lock()
            .doom(Some(crate::metal::hart::hart_id()), cause, reason);
    }
}

/// Fetch only resident executable bytes through Nest; never dereference a user PC.
fn instruction(space: &Nest, pc: KVirt) -> Option<u32> {
    let mut word = u32::from(space.instruction_byte(pc)?);
    let second = pc.as_usize().checked_add(1)?;
    word |= u32::from(space.instruction_byte(KVirt::wrap(second))?) << 8;
    if word & 3 == 3 {
        for offset in 2..4 {
            let address = pc.as_usize().checked_add(offset)?;
            word |= u32::from(space.instruction_byte(KVirt::wrap(address))?) << (offset * 8);
        }
    }
    Some(word)
}
