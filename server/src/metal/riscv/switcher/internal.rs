//! Trusted Kernel-Team scheduling boundary. This is an ordinary supervisor
//! function call, never an exception or a public Kernel ABI operation.
use super::context::TrapContext;

/// Save the supervisor Rust call's continuation before leaving its Task stack.
/// Callers must drop every guard and resource reference before invoking this.
/// The function ABI permits caller-saved GPRs to be clobbered; Kernel tasks do
/// not use floating/vector state. SIE is masked before touching the saved frame.
#[unsafe(naked)]
unsafe extern "C" fn suspend(_at: u64, _exit: usize) {
    core::arch::naked_asm!(
        "csrrci t0, sstatus, 2",
        "csrr t1, sscratch",
        "sd ra, 0x38(t1)",
        "sd ra, 0x138(t1)",
        "sd sp, 0x40(t1)",
        "sd gp, 0x48(t1)",
        "sd tp, 0x50(t1)",
        "sd s0, 0x70(t1)",
        "sd s1, 0x78(t1)",
        "sd s2, 0xc0(t1)",
        "sd s3, 0xc8(t1)",
        "sd s4, 0xd0(t1)",
        "sd s5, 0xd8(t1)",
        "sd s6, 0xe0(t1)",
        "sd s7, 0xe8(t1)",
        "sd s8, 0xf0(t1)",
        "sd s9, 0xf8(t1)",
        "sd s10, 0x100(t1)",
        "sd s11, 0x108(t1)",
        "sd a0, 0x80(t1)",
        "sd a1, 0x88(t1)",
        // Restore SIE through sret's SPIE, preserving the supervisor origin.
        "andi t2, t0, 2",
        "slli t2, t2, 4",
        "andi t0, t0, -35",
        "or t0, t0, t2",
        "ori t0, t0, 256",
        "sd t0, 0x130(t1)",
        "ld sp, 0x08(t1)",
        "ld a0, 0x20(t1)",
        "tail {schedule}",
        schedule = sym schedule,
    );
}

extern "C" fn schedule(frame: &mut TrapContext) -> ! {
    // Nested IRQ continuations belong to the Hart frame, even if a completion
    // poll temporarily admits interrupts while scheduling on the trap stack.
    unsafe {
        core::arch::asm!("csrw sscratch, {}", in(reg) crate::metal::hart::hart_frame().as_usize(), options(nostack));
    }
    let task = crate::basis::room::scheduler::slot::current()
        .running_task()
        .expect("internal scheduling without Task");
    assert!(task.ident.team.nest.asid().is_kernel());
    assert!(task.ident.team.nest.kind().is_supervisor());
    assert_eq!(task.ident.frame_pa.as_usize(), frame as *mut _ as usize);
    let action = frame.gpr.x(super::context::Gprs::A1);
    let at = frame.gpr.x(super::context::Gprs::A0) as u64;
    let id = task.ident.id.get();
    drop(task);
    let next = if action == 1 {
        crate::basis::unit::lifecycle::set_exit_reason(0);
        crate::basis::unit::lifecycle::quit()
    } else if action == 2 {
        let work = if crate::basis::unit::lifecycle::WORK.identifies(id) {
            &crate::basis::unit::lifecycle::WORK
        } else {
            let retirement = &crate::basis::nest::retirement::WORK;
            assert!(retirement.identifies(id), "resident wait owner");
            retirement
        };
        match work.wait() {
            crate::basis::room::Handoff::Resume(()) => frame as *mut _ as usize,
            crate::basis::room::Handoff::Switch(pa) => pa,
        }
    } else if at == 0 {
        assert_eq!(action, 0, "internal scheduling action");
        crate::basis::room::scheduler::slot::current().reschedule()
    } else {
        crate::basis::sign::park_until::<kernel_api::abi::call::room::RoomFail>(at)
            .expect("trusted observer sleep")
            .unwrap_or(frame as *mut _ as usize)
    };
    super::trampoline::restore(next)
}

/// A zero deadline yields once; a nonzero deadline waits until that instant.
pub(crate) fn sleep(at: u64) {
    // SAFETY: private callers are Kernel Team tasks at an explicit, guard-free
    // scheduling boundary. The selected context returns through sret.
    unsafe { suspend(at, 0) }
}

/// Park a registered maintenance Task on its allocation-free retained wake Site.
/// The saved ordinary-call continuation resumes after this boundary.
pub(crate) fn wait() {
    unsafe { suspend(0, 2) }
}

pub(crate) fn exit() -> ! {
    unsafe { suspend(0, 1) };
    unreachable!("exited Kernel Task resumed")
}
