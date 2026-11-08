mod fault;
pub(crate) mod resources;

use alloc::sync::Arc;
use riscv::interrupt::Exception;
use riscv::interrupt::Interrupt;
use riscv::interrupt::Trap;
use riscv::register::scause;
use riscv::register::sepc;
use riscv::register::sie;
use riscv::register::sip;
use riscv::register::stval;

use crate::basis::nest::FaultOutcome;
use crate::basis::nest::Nest;
use crate::basis::room::conductor::identity::Identity;
use crate::basis::room::conductor::identity::ident;
use crate::basis::room::conductor::run::run;
use crate::basis::room::deadline;
use crate::basis::sign::redeem;
use crate::basis::unit::task::TaskIdent;
use crate::machine::hart;
use crate::machine::hart::HartId;
use crate::machine::mmu::riscv::identity as asid;
use crate::machine::mmu::riscv::identity::Asid;
use crate::machine::switcher::context::TrapContext;
use crate::putln;
use kernel_api::address::VirtAddr as KVirt;

use crate::machine::switcher::stack::TRAP_STACK_CANARY;
use crate::machine::switcher::stack::trap_stack_base;
#[cfg(debug_assertions)]
use crate::machine::switcher::stack::trap_stack_edge;
use crate::machine::switcher::stack::trap_stack_guard_hart;
use crate::machine::switcher::stack::trap_stack_hart;

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
        (*dst).gpr = frame.gpr;
        (*dst).sstatus = frame.sstatus;
        (*dst).sepc = frame.sepc;
    }
    true
}

fn hart_frame_pa() -> usize {
    unsafe {
        (*(hart::hart_frame().as_usize() as *const TrapContext))
            .user_pa
            .as_usize()
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn trap_handler(frame: &mut TrapContext) -> *mut TrapContext {
    let sp: usize;
    unsafe {
        ::core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    let hart = trap_stack_hart(sp).unwrap_or(HartId::new(0));
    let tp = crate::machine::hart::per_hart_ptr(hart);
    unsafe {
        ::core::arch::asm!("mv tp, {}", in(reg) tp, options(nomem, nostack, preserves_flags));
    }

    let frame_va = crate::machine::hart::hart_frame().as_usize();
    unsafe {
        ::core::arch::asm!(
            "csrw sscratch, {}",
            in(reg) frame_va,
            options(nomem, nostack, preserves_flags)
        );
    }

    asid::occupy(Asid::kernel());

    let from_task = (frame as *const TrapContext as usize) != hart_frame_pa();

    if from_task {
        let task = crate::basis::room::conductor::current()
            .running_task()
            .expect("task trap without owner");
        assert_eq!(task.ident.frame_pa.as_usize(), frame as *const _ as usize);
        unsafe { task.capture_extensions(frame.sstatus.bits()) };
    }
    let ident = ident();

    crate::machine::stop::hush();

    let cause = scause::read();
    if cause.is_exception() && matches!(cause.code(), 12 | 13 | 15) {
        let stv = stval::read();
        if let Some(h) = trap_stack_guard_hart(stv) {
            panic!("trap stack overflow on hart {h} (stval = {stv:#x})");
        }
    }

    let me = hart::hart_id();
    let canary = unsafe { (trap_stack_base(me).as_usize() as *const usize).read() };
    assert_eq!(
        canary, TRAP_STACK_CANARY,
        "trap stack corrupted on hart {me} (overflow?)"
    );
    assert_eq!(
        frame.trap_stack_corrupt, TRAP_STACK_CANARY,
        "kernel trap frame corrupted"
    );
    #[cfg(debug_assertions)]
    if let Some(i) = ident.as_ref()
        && from_task
    {
        let sp: usize;
        unsafe {
            ::core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags));
        }
        let top = trap_stack_edge(me).as_usize();
        let ksp = frame.kernel_sp.as_usize();
        debug_assert!(
            sp <= top && top - sp < 0x4000,
            "user trap on hart {me}: sp={sp:#x} top={top:#x} frame.kernel_sp={ksp:#x} (task #{}) — kernel_sp per-switch write missing?",
            i.task_id()
        );
    }

    let trap: Trap<Interrupt, Exception> = scause::read().cause().try_into().unwrap_or_else(|e| {
        panic!("unknown trap cause: {e:?}");
    });
    let next: *mut TrapContext = match trap {
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            deadline::tick();
            crate::basis::cell::projection::maintain();
            unsafe {
                sie::set_sext();
            }
            deadline::beat_until(deadline::blind_ceiling());
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
                frame as *mut TrapContext
            }
        }
        Trap::Interrupt(Interrupt::SupervisorSoft) => {
            unsafe {
                sip::clear_ssoft();
            }
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
        Trap::Interrupt(Interrupt::SupervisorExternal) => {
            if crate::machine::iommu::riscv::interrupt::handle() {
                crate::basis::cell::projection::maintain();
            } else {
                let result = crate::basis::sign::endpoint::trigger(
                    &crate::basis::room::interrupt::get().supervisor_external,
                );
                crate::basis::room::interrupt::note_external(result.is_err(), false);
                if result.is_err() {
                    unsafe {
                        sie::clear_sext();
                    }
                }
            }
            frame as *mut TrapContext
        }
        Trap::Exception(
            Exception::Breakpoint | Exception::UserEnvCall | Exception::SupervisorEnvCall,
        ) => {
            if !from_task {
                panic!("kernel ebreak from the kernel itself");
            }
            match envcall(frame, ident) {
                Some(next) => next,
                None => crate::basis::unit::lifecycle::quit() as *mut TrapContext,
            }
        }
        Trap::Exception(
            Exception::InstructionPageFault | Exception::LoadPageFault | Exception::StorePageFault,
        ) => {
            if !from_task {
                panic!(
                    "kernel page fault on hart {} at sepc={:#x}, stval={:#x}",
                    hart::hart_id(),
                    sepc::read(),
                    stval::read()
                );
            }
            let fault = unsafe { fault::PageFault::capture() };
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
                            fault.pc
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
        Trap::Exception(Exception::IllegalInstruction) if from_task => {
            let task = crate::basis::room::conductor::current()
                .running_task()
                .expect("task fault without owner");
            let space = &task.ident.team.nest;
            let instruction = instruction(space, frame.sepc);
            let mut status = frame.sstatus.bits();
            let enabled = instruction.map(|word| task.enable_extension(word, &mut status));
            drop(task);
            match enabled {
                Some(Ok(true)) => {
                    frame.sstatus = riscv::register::sstatus::Sstatus::from_bits(status);
                    frame as *mut TrapContext // Retry the same PC, including compressed instructions.
                }
                Some(Err(())) => {
                    log::error!("Task extended context allocation failed");
                    return kill_fault(ident);
                }
                _ => return kill_fault(ident),
            }
        }
        Trap::Exception(other) => {
            if from_task {
                let running = ident
                    .as_ref()
                    .and_then(Identity::live)
                    .expect("user exception without running task");
                let tid = running.id.get();
                let stval_bits = stval::read();
                putln!(
                    "user exception killed: tid={tid} cause={:?} stval={stval_bits:#x}",
                    other
                );
                return kill_fault(ident);
            }
            panic!(
                "unhandled kernel exception: {other:?} at sepc={:#x}, stval={:#x}",
                sepc::read(),
                stval::read()
            );
        }
    };

    let me = hart::hart_id();
    let canary = unsafe { (trap_stack_base(me).as_usize() as *const usize).read() };
    assert_eq!(
        canary, TRAP_STACK_CANARY,
        "trap stack corrupted on hart {me} after handler"
    );

    resume(next)
}

pub(crate) fn resume(next: *mut TrapContext) -> *mut TrapContext {
    if unsafe { (*next).user_satp.asid() } != 0 {
        let task = crate::basis::room::conductor::current()
            .running_task()
            .expect("return without Task owner");
        assert_eq!(task.ident.frame_pa.as_usize(), next as usize);
        let mut status = unsafe { (*next).sstatus.bits() };
        unsafe {
            task.restore_extensions(&mut status);
            (*next).sstatus = riscv::register::sstatus::Sstatus::from_bits(status);
        }
    }
    asid::occupy(Asid::from_raw(unsafe { (*next).user_satp.asid() }));
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
            crate::basis::room::scheduler::roster::muster(ident.id).and_then(|weak| weak.upgrade())
        }
        None => None,
    }
    .or_else(|| crate::basis::room::conductor::current().running_task());
    let Some(task) = task else { return };
    let _commit = crate::basis::commit();
    crate::basis::unit::team::exit::member_dying(&task, reason);
    if !task.dead() {
        task.state
            .lock()
            .doom(Some(crate::machine::hart::hart_id()), cause, reason);
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
