use core::sync::atomic::{AtomicU32, Ordering};
use kernel_api::{
    address::VirtAddr,
    model::{unit::UnitTarget, wait::Wait},
};
use kernel_client::call::{room, unit};
static VALUE: AtomicU32 = AtomicU32::new(0);
static ACK: AtomicU32 = AtomicU32::new(0);
fn address(value: &AtomicU32) -> VirtAddr {
    VirtAddr::new(value as *const _ as usize)
}
fn until(value: &AtomicU32, wanted: u32) {
    let deadline = room::clock().saturating_add(1_000_000_000);
    loop {
        let old = value.load(Ordering::Acquire);
        if old >= wanted {
            return;
        }
        assert!(room::clock() < deadline, "lost value wake");
        room::wait_value(address(value), old, Wait::At(deadline)).unwrap();
    }
}
extern "C" fn recipient() -> ! {
    for round in 1..=32 {
        until(&VALUE, round);
        ACK.store(round, Ordering::Release);
        room::wake_value(address(&ACK), true).unwrap();
    }
    unit::exit(0, 0, VirtAddr::new(0), 0)
}
pub(super) fn run() {
    assert!(!room::wait_value(address(&VALUE), 0, Wait::Poll).unwrap());
    assert!(room::wait_value(address(&VALUE), 1, Wait::Poll).unwrap());
    let task = super::lifecycle::spawn(recipient as *const () as usize);
    unit::resume(UnitTarget::Task(task)).unwrap();
    for round in 1..=32 {
        VALUE.store(round, Ordering::Release);
        room::wake_value(address(&VALUE), true).unwrap();
        until(&ACK, round);
    }
    assert!(super::lifecycle::wait_task(task));
    super::lifecycle::delete(task);
}
