use riscv::interrupt::{Exception, Interrupt, Trap};
use riscv::register::{scause, sepc, stval};

use crate::metal::generic::trap::{Fault, FaultKind, PageFault, TrapEvent};
use crate::metal::hart::{self, HartId};
use crate::metal::mmu::canonical;

use super::context::TrapContext;
use super::stack::{TRAP_STACK_CANARY, trap_stack_base, trap_stack_guard_hart, trap_stack_hart};

pub(crate) fn capture() -> TrapEvent {
    let cause = scause::read();
    let trap: Trap<Interrupt, Exception> = cause
        .cause()
        .try_into()
        .unwrap_or_else(|error| panic!("unknown trap cause: {error:?}"));
    match trap {
        Trap::Interrupt(Interrupt::SupervisorTimer) => TrapEvent::Timer,
        Trap::Interrupt(Interrupt::SupervisorSoft) => TrapEvent::Ipi,
        Trap::Interrupt(Interrupt::SupervisorExternal) => TrapEvent::External,
        Trap::Exception(
            Exception::Breakpoint | Exception::UserEnvCall | Exception::SupervisorEnvCall,
        ) => TrapEvent::Call,
        Trap::Exception(exception) => {
            let detail = Fault {
                cause: cause.bits(),
                value: stval::read(),
                pc: sepc::read(),
            };
            let kind = match exception {
                Exception::InstructionPageFault => FaultKind::Instruction,
                Exception::LoadPageFault => FaultKind::Load,
                Exception::StorePageFault => FaultKind::Store,
                Exception::IllegalInstruction => return TrapEvent::IllegalInstruction(detail),
                _ => return TrapEvent::Exception(detail),
            };
            if let Some(hart) = trap_stack_guard_hart(detail.value) {
                panic!(
                    "trap stack overflow on hart {hart} (stval={:#x})",
                    detail.value
                );
            }
            TrapEvent::PageFault(PageFault {
                addr: canonical(detail.value),
                detail,
                kind,
            })
        }
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn trap_handler(frame: &mut TrapContext) -> *mut TrapContext {
    let from_task = enter(frame);
    crate::entry::trap::handle(frame, from_task)
}

/// Restore hart-local addressing before any Rust code uses it. This runs with
/// interrupts masked on the trampoline's stack and kernel page table.
#[inline(always)]
fn enter(frame: &TrapContext) -> bool {
    let sp: usize;
    unsafe {
        core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    let hart = trap_stack_hart(sp).unwrap_or(HartId::new(0));
    let tp = hart::per_hart_ptr(hart);
    unsafe {
        core::arch::asm!("mv tp, {}", in(reg) tp, options(nomem, nostack, preserves_flags));
    }
    let hart_frame = hart::hart_frame().as_usize();
    unsafe {
        core::arch::asm!("csrw sscratch, {}", in(reg) hart_frame, options(nomem, nostack, preserves_flags));
    }
    let hart_frame_pa = unsafe { (*(hart_frame as *const TrapContext)).user_pa };
    frame as *const TrapContext as usize != hart_frame_pa.as_usize()
}

pub(crate) fn check_stack() {
    let me = hart::hart_id();
    let canary = unsafe { (trap_stack_base(me).as_usize() as *const usize).read() };
    assert_eq!(
        canary, TRAP_STACK_CANARY,
        "trap stack corrupted on hart {me} (overflow?)"
    );
}

#[cfg(debug_assertions)]
pub(crate) fn check_task_stack(frame: &TrapContext, task: usize) {
    let sp: usize;
    unsafe {
        core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags));
    }
    let me = hart::hart_id();
    let top = super::stack::trap_stack_edge(me).as_usize();
    let ksp = frame.kernel_sp.as_usize();
    debug_assert!(
        sp <= top && top - sp < 0x4000,
        "user trap on hart {me}: sp={sp:#x} top={top:#x} frame.kernel_sp={ksp:#x} (task #{task}) — kernel_sp per-switch write missing?",
    );
}
