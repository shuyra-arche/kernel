//! Page-completion discovery on an independent Kernel Team Task. The bounded
//! source cursor stays with Nest, and Page admission/retirement stays with Cell.
use crate::basis::nest::retirement::{self, Progress, WORK};
use crate::basis::unit::{task::Task, team::kernel};
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
        let stopping = crate::basis::room::shutdown::active();
        if stopping {
            break;
        }
        match retirement::advance() {
            Progress::Ready => super::yield_now(),
            Progress::Idle => super::wait(),
            Progress::Blocked => {
                WORK.defer();
                super::wait();
            }
        }
    }
    WORK.close();
    super::exit()
}
