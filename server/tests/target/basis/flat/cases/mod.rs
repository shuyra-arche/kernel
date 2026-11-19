use crate::basis::unit::{task::Task, team::kernel};
use crate::entry::boot::BootError;
use crate::metal::{clock, hart};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use kernel_api::address::VirtAddr;

mod contracts;

pub(crate) fn start() -> Result<(), BootError> {
    contracts::prepare()?;
    let entry = contracts::run as *const () as usize;
    let task = kernel()
        .expect("kernel team")
        .task()
        .entry(VirtAddr::new(entry))
        .hold()
        .map_err(BootError::Bootstrap)?;
    crate::basis::room::shutdown::watch(&task);
    Task::release(&task).map_err(|source| BootError::BootstrapRelease {
        task: task.ident.id,
        source,
    })
}

fn now() -> u64 {
    clock::now().as_ticks()
}
fn note(args: core::fmt::Arguments<'_>) {
    use core::fmt::Write;
    let mut out = crate::metal::console::Emergency::new();
    core::fmt::write(&mut out, args).unwrap();
    out.write_str("\n").unwrap();
    out.finish();
}
fn yield_now() {
    crate::metal::switcher::internal::sleep(0);
}
fn exit() -> ! {
    crate::metal::switcher::internal::exit()
}

struct Pin(bool);
impl Pin {
    fn new() -> Self {
        let enabled = riscv::register::sstatus::read().sie();
        unsafe { riscv::register::sstatus::clear_sie() };
        Self(enabled)
    }
}
impl Drop for Pin {
    fn drop(&mut self) {
        if self.0 {
            unsafe { riscv::register::sstatus::set_sie() };
        }
    }
}
