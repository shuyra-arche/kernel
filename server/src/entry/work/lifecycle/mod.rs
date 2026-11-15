//! Unit cleanup's independent Task. Its owner retains queues and retirement
//! evidence; one turn handles at most four pending exits, four doom steps and
//! four husks. Hardware Busy sleeps on a retained timed notification.
use crate::basis::unit::{
    lifecycle::{self, Progress, WORK},
    task::Task,
    team::kernel,
};
use crate::entry::boot::BootError;
use kernel_api::address::VirtAddr;
pub(crate) fn start() -> Result<(), BootError> {
    let task = kernel()
        .expect("Kernel Team")
        .task()
        .entry(VirtAddr::new(run as *const () as usize))
        .hold()
        .map_err(BootError::Bootstrap)?;
    WORK.bind(&task).map_err(BootError::Bootstrap)?;
    Task::release(&task).map_err(|source| BootError::BootstrapRelease {
        task: task.ident.id,
        source,
    })
}
extern "C" fn run() -> ! {
    loop {
        let stopping = {
            let _commit = crate::basis::commit();
            crate::basis::room::shutdown::active()
        };
        if stopping {
            break;
        }
        let progress = {
            let _local = unsafe { crate::metal::sync::TrapGuard::save() };
            lifecycle::advance()
        };
        match progress {
            Progress::Ready => super::yield_now(),
            Progress::Blocked => {
                WORK.defer();
                super::wait();
            }
            Progress::Idle => super::wait(),
        }
    }
    WORK.close();
    super::exit()
}
