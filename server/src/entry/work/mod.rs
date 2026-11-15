//! Resident Kernel Team Tasks, grouped by independently scheduled responsibility.
//! Boot constructs and starts each Task explicitly; this module is not a dispatcher.
//! Keep entry, startup, wake sources and bounded loop together in each child.
//! Authoritative resource state and retirement evidence remain with basis owners.
pub(crate) mod observe;

pub(crate) mod lifecycle;
use crate::metal::switcher::context::{Gprs, TrapContext};
const PARK: usize = usize::MAX;
pub(crate) fn park(frame: &mut TrapContext, id: usize) -> Option<*mut TrapContext> {
    if frame.gpr.x(Gprs::A7) != PARK || !crate::basis::unit::lifecycle::WORK.identifies(id) {
        return None;
    }
    Some(match crate::basis::unit::lifecycle::WORK.wait() {
        crate::basis::room::Handoff::Resume(()) => frame as *mut _,
        crate::basis::room::Handoff::Switch(pa) => pa as *mut _,
    })
}
fn wait() {
    unsafe {
        core::arch::asm!("ebreak", in("a7") PARK, lateout("a0") _, lateout("a1") _, lateout("a2") _, options(nostack));
    }
}
fn yield_now() {
    use kernel_api::abi::{call::room::operations::Yield, operation::RegisterCall};
    unsafe {
        core::arch::asm!("ebreak", in("a7") Yield::SLOT, lateout("a0") _, lateout("a1") _, lateout("a2") _, options(nostack));
    }
}
fn exit() -> ! {
    use kernel_api::abi::{call::unit::operations::Exit, operation::RegisterCall};
    unsafe {
        core::arch::asm!("ebreak", in("a0") 0usize, in("a1") 0usize, in("a2") 0usize, in("a3") 0usize, in("a4") 0usize, in("a7") Exit::SLOT, options(noreturn));
    }
}
