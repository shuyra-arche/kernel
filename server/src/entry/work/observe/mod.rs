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
    let mut next_global = 0u64;
    loop {
        let stopping = crate::basis::room::shutdown::active();
        if stopping {
            break;
        }
        if now() >= next_global {
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
            next_global = now().saturating_add(100_000_000);
        }
        crate::basis::nest::activity::advance();
        sleep(
            now().saturating_add(if crate::basis::nest::activity::active() {
                10_000_000
            } else {
                100_000_000
            }),
        );
    }
    exit()
}
fn sleep(at: u64) {
    // No guards or resource references survive this scheduling boundary.
    crate::metal::switcher::internal::sleep(at);
}
fn exit() -> ! {
    crate::metal::switcher::internal::exit()
}
