#[cfg(feature = "clock")]
pub(super) mod counter;

use core::sync::atomic::{AtomicU32, Ordering};
use kernel_api::{
    address::VirtAddr,
    model::{unit::UnitTarget, wait::Wait},
};
use kernel_client::call::{room, unit};
static VALUE: AtomicU32 = AtomicU32::new(0);
static ACK: AtomicU32 = AtomicU32::new(0);
static ALARM: AtomicU32 = AtomicU32::new(0);
fn address(value: &AtomicU32) -> VirtAddr {
    VirtAddr::new(value as *const _ as usize)
}
/// One condition-wait attempt. A concurrent mapping invalidation may reject
/// the atomic read with Busy; retain the caller's value/deadline and recheck.
pub(super) fn wait(value: &AtomicU32, expected: u32, until: Wait) {
    match room::wait_value(address(value), expected, until) {
        Ok(_) => {}
        Err(error) if error.source == kernel_api::abi::call::room::RoomFail::Busy => room::yield_(),
        Err(error) => panic!("value wait: {error:?}"),
    }
}
fn until(value: &AtomicU32, wanted: u32) {
    let deadline = room::clock().saturating_add(1_000_000_000);
    loop {
        let old = value.load(Ordering::Acquire);
        if old >= wanted {
            return;
        }
        assert!(room::clock() < deadline, "lost value wake");
        wait(value, old, Wait::At(deadline));
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
extern "C" fn earlier_alarm() -> ! {
    for round in 1..=16 {
        until(&VALUE, 32 + round);
        // Let the peer enter its long wait first. Its deadline must not
        // determine how long this Hart's subsequent short wait takes.
        room::sleep_until(room::clock() + 5_000_000).unwrap();
        let short = room::clock() + 2_000_201;
        room::sleep_until(short).unwrap();
        assert!(room::clock() >= short, "early deadline");
        ALARM.store(round, Ordering::Release);
        room::wake_value(address(&ALARM), true).unwrap();
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

    let task = super::lifecycle::spawn(earlier_alarm as *const () as usize);
    unit::resume(UnitTarget::Task(task)).unwrap();
    for round in 1..=16 {
        VALUE.store(32 + round, Ordering::Release);
        room::wake_value(address(&VALUE), true).unwrap();
        until(&ALARM, round);
    }
    assert!(super::lifecycle::wait_task(task));
    super::lifecycle::delete(task);
    room::sleep_until(0).unwrap();
    assert!(!room::wait_value(address(&ALARM), 16, Wait::At(0)).unwrap());
}
