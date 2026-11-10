//! A resident Kernel Team task samples fixed-size observations. Producers own
//! the facts; stopping this task cannot stop allocation or hardware retirement.
mod resources;
use crate::basis::unit::{task::Task, team::kernel};
use crate::entry::boot::BootError;
use kernel_api::address::VirtAddr;
use kernel_api::model::memory::{Snapshot, SnapshotPage, UNKNOWN};
static PAGE: SnapshotPage = SnapshotPage::new();
pub(crate) use resources::{identifies, register};
pub(crate) fn start() -> Result<(), BootError> {
    let task = kernel()
        .expect("kernel team")
        .task()
        .entry(VirtAddr::new(run as *const () as usize))
        .hold()
        .map_err(BootError::Bootstrap)?;
    Task::release(&task).map_err(|source| BootError::BootstrapRelease {
        task: task.ident.id,
        source,
    })
}
fn now() -> u64 {
    crate::metal::clock::uptime()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}
extern "C" fn run() -> ! {
    let mut epoch = 0u64;
    loop {
        // Shutdown lookup temporarily holds a Hart-tracked TaskWeak. Unlike a
        // trap handler, this ordinary Kernel Task runs with interrupts enabled:
        // keep that lookup on its originating Hart until all copies drop. No
        // guard or Task reference may cross the explicit SleepUntil below.
        let stopping = {
            let _commit = crate::basis::commit();
            crate::basis::room::shutdown::active()
        };
        if stopping {
            break;
        }
        let Some(next) = epoch.checked_add(1) else {
            break;
        };
        epoch = next;
        let start_ns = now();
        let (capacity, available, resident) = crate::metal::allocator::frame::snapshot();
        let retained = crate::basis::cell::page::statistics::snapshot();
        let snapshot = Snapshot {
            epoch,
            start_ns,
            end_ns: now(),
            capacity,
            available,
            resident,
            cpu_retained: retained.cpu,
            io_retained: retained.external,
            retiring: retained.retiring,
            abandoned: retained.abandoned,
            accessed: UNKNOWN,
            dirty: UNKNOWN,
        };
        if !PAGE.publish(snapshot) {
            break;
        }
        resources::notify();
        sleep(now().saturating_add(100_000_000));
    }
    exit()
}
fn sleep(at: u64) {
    use kernel_api::abi::{call::room::operations::SleepUntil, operation::RegisterCall};
    let mut result = at as usize;
    unsafe {
        core::arch::asm!("ebreak", inlateout("a0") result, in("a7") SleepUntil::SLOT, lateout("a1") _, lateout("a2") _, options(nostack));
    }
    let _ = result;
}
fn exit() -> ! {
    use kernel_api::abi::{call::unit::operations::Exit, operation::RegisterCall};
    unsafe {
        core::arch::asm!("ebreak", in("a0") 0usize, in("a1") 0usize, in("a2") 0usize, in("a3") 0usize, in("a4") 0usize, in("a7") Exit::SLOT, options(noreturn));
    }
}
