//! Protection changes preserve another association's frozen snapshot.
use super::*;
use core::fmt::Write;
use kernel_api::{
    abi::call::cell::CellFail,
    model::cell::{CellObject, CellState},
};
use kernel_client::call::cell;

fn frozen_alias() {
    let slot = crate::allocate(1);
    let (a, _) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        6,
    );
    let (b, address) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        6,
    );
    for token in [a, b] {
        crate::handoff(crate::slot(slot, 0), crate::memo(token, 0), false);
        cell::protect(crate::ptr(&crate::memo(token, 0).to_bytes()), 2).unwrap();
        crate::publish(token, 0);
    }
    assert_eq!(crate::read(address, 0), 0);
    crate::retract(a, 0);
    let frozen = crate::info(crate::raw_memo(a, 0));
    let published = crate::info(crate::raw_memo(b, 0));
    assert_eq!(frozen.state, CellState::Ready);
    assert!(frozen.frozen);
    assert_eq!(published.state, CellState::Published);
    assert_eq!(
        cell::protect(crate::ptr(&crate::memo(b, 0).to_bytes()), 6)
            .unwrap_err()
            .source,
        CellFail::Busy
    );
    assert_eq!(crate::info(crate::raw_memo(a, 0)), frozen);
    assert_eq!(crate::info(crate::raw_memo(b, 0)), published);
    cell::remove(crate::ptr(&crate::memo(a, 0).to_bytes())).unwrap();
    assert_eq!(crate::info(crate::raw_memo(a, 0)).state, CellState::Vacant);
    cell::protect(crate::ptr(&crate::memo(b, 0).to_bytes()), 6).unwrap();
    assert_eq!(
        crate::info(crate::raw_memo(b, 0)).state,
        CellState::Published
    );
    crate::write(address, 0, 0xabcdef12);
    assert_eq!(crate::read(address, 0), 0xabcdef12);
    crate::retract(b, 0);
    for token in [a, b] {
        crate::lifecycle::return_pend(token);
    }
    crate::delete(CellObject::Slot(slot));
}

fn instruction(address: VirtAddr, value: u32) {
    unsafe {
        (address.get() as *mut u32).write_volatile((value << 20) | 0x513);
        ((address.get() + 4) as *mut u32).write_volatile(0x00008067);
    }
}

fn execute(address: VirtAddr) -> usize {
    let function: extern "C" fn() -> usize = unsafe { core::mem::transmute(address.get()) };
    function()
}

fn executable_update() {
    let slot = crate::allocate(1);
    let (mapping, address) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        14,
    );
    crate::handoff(crate::slot(slot, 0), crate::memo(mapping, 0), false);
    cell::protect(crate::ptr(&crate::memo(mapping, 0).to_bytes()), 6).unwrap();
    crate::publish(mapping, 0);
    instruction(address, 17);
    cell::protect(crate::ptr(&crate::memo(mapping, 0).to_bytes()), 10).unwrap();
    assert_eq!(execute(address), 17);
    cell::protect(crate::ptr(&crate::memo(mapping, 0).to_bytes()), 6).unwrap();
    instruction(address, 29);
    cell::protect(crate::ptr(&crate::memo(mapping, 0).to_bytes()), 10).unwrap();
    // No guest FENCE.I: QEMU checks functional execution, not physical I-cache coherence.
    assert_eq!(execute(address), 29);
    crate::retract(mapping, 0);
    crate::lifecycle::return_pend(mapping);
    crate::delete(CellObject::Slot(slot));
}

pub(super) fn run() {
    frozen_alias();
    executable_update();
    writeln!(
        crate::Console,
        "PASS nest-protection foreign-freeze=1 busy-unchanged=1 remove-thaw=1 execution-update=1"
    )
    .unwrap();
}
