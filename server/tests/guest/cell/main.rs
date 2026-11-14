#![no_std]
#![no_main]
#[cfg(feature = "context")]
mod context;
mod external;
#[cfg(feature = "irq")]
mod interrupt;
mod levels;
mod license;
mod lifecycle;
mod observe;
mod organization;
mod pend;
#[cfg(feature = "realtime")]
mod realtime;
mod room;
mod sign;
mod space;
use core::fmt::Write;
use kernel_api::model::unit::ExitScope;
use kernel_api::{address::VirtAddr, model::cell::*};
use kernel_client::call::{cell, debug, nest, room as room_call, unit};
core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail run"
);
struct Console;
impl core::fmt::Write for Console {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let mut bytes = text.as_bytes();
        let deadline = kernel_client::call::room::clock().saturating_add(2_000_000_000);
        while !bytes.is_empty() {
            let size = bytes.len().min(256);
            let n = debug::put(VirtAddr::new(bytes.as_ptr() as usize), size)
                .map_err(|_| core::fmt::Error)?;
            if n == 0 {
                if kernel_client::call::room::clock() >= deadline {
                    return Err(core::fmt::Error);
                }
                kernel_client::call::room::yield_();
                continue;
            }
            bytes = &bytes[n..];
        }
        Ok(())
    }
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL cell-kernel {info}");
    unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn ptr(bytes: &[u8]) -> VirtAddr {
    VirtAddr::new(bytes.as_ptr() as usize)
}
// An unrelated Task may own this Nest's fence. Busy retains no caller-owned
// address result; retry across a scheduling boundary under a fixed deadline.
fn open_share(
    token: kernel_api::model::identity::PendId,
) -> kernel_api::abi::call::share_view::ShareResult<(VirtAddr, usize)> {
    let deadline = room_call::clock().saturating_add(5_000_000_000);
    loop {
        match kernel_client::call::share_view::open(token) {
            Err(error) if error.source == kernel_api::abi::call::share_view::ShareFail::Busy => {
                assert!(
                    room_call::clock() < deadline,
                    "Share.Open continuation timed out"
                );
                room_call::yield_();
            }
            result => return result,
        }
    }
}
fn allocate(count: usize) -> SlotId {
    cell::create(
        crate::license::get(),
        Allocation::new(count * 4096, 4096).unwrap(),
    )
    .unwrap()
}
fn reserve(reference: SlotId, count: usize) -> (kernel_api::model::identity::PendId, VirtAddr) {
    let level = info(slot(reference, 0)).level;
    space::reserve(
        space::get(),
        Placement::lowest(level.bytes().unwrap()).unwrap(),
        count,
        level,
        6,
    )
}
fn slot(reference: SlotId, page: usize) -> CellPosition {
    CellPosition {
        object: CellObject::Slot(reference),
        page,
        generation: 0,
    }
}
fn raw_memo(mapping: kernel_api::model::identity::PendId, page: usize) -> CellPosition {
    CellPosition {
        object: CellObject::Pend(mapping),
        page,
        generation: 0,
    }
}
#[track_caller]
fn info(pos: CellPosition) -> CellInfo {
    let position = pos.to_bytes();
    let mut bytes = [0; CellInfo::WIDTH];
    cell::inspect(ptr(&position), VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
    CellInfo::from_bytes(&bytes).unwrap()
}
fn memo(mapping: kernel_api::model::identity::PendId, page: usize) -> CellPosition {
    let mut pos = raw_memo(mapping, page);
    pos.generation = info(pos).generation;
    pos
}
#[track_caller]
fn handoff(source: CellPosition, target: CellPosition, pull: bool) {
    if matches!(target.object, CellObject::Pend(_)) {
        cell::insert(ptr(&CellAssociation { source, target }.to_bytes()))
    } else if pull {
        cell::pull(ptr(&CellHandoff {
            local: target,
            remote: source,
        }
        .to_bytes()))
    } else {
        cell::push(ptr(&CellHandoff {
            local: source,
            remote: target,
        }
        .to_bytes()))
    }
    .unwrap_or_else(|error| panic!("handoff {source:?} -> {target:?}: {error:?}"));
}
#[track_caller]
fn publish(mapping: kernel_api::model::identity::PendId, page: usize) {
    let deadline = room_call::clock().saturating_add(5_000_000_000);
    loop {
        let current = info(raw_memo(mapping, page)); // Also progresses accepted retirement.
        if current.state == CellState::Published {
            break;
        }
        let pos = CellPosition {
            generation: current.generation,
            ..raw_memo(mapping, page)
        }
        .to_bytes();
        match cell::publish(ptr(&pos)) {
            Ok(()) => break,
            Err(error) if error.source == kernel_api::abi::call::cell::CellFail::Busy => {
                assert!(
                    room_call::clock() < deadline,
                    "Cell.Publish continuation timed out"
                );
                room_call::yield_();
            }
            Err(error) => panic!("Cell.Publish: {error:?}"),
        }
    }
    assert_eq!(info(raw_memo(mapping, page)).state, CellState::Published);
}
#[track_caller]
fn retract(mapping: kernel_api::model::identity::PendId, page: usize) {
    let pos = memo(mapping, page);
    let before = info(pos);
    assert_eq!(before.state, CellState::Published);
    let next = before.generation.checked_add(1).unwrap();
    let deadline = room_call::clock().saturating_add(5_000_000_000);
    let mut accepted = false;
    loop {
        if !accepted {
            match cell::retract(ptr(&pos.to_bytes())) {
                Ok(()) => accepted = true,
                Err(error) if error.source == kernel_api::abi::call::cell::CellFail::Busy => (),
                Err(error) => panic!("Cell.Retract: {error:?}"),
            }
        }
        let current = info(raw_memo(mapping, page));
        assert_eq!(current.page, before.page);
        if current.generation == next {
            accepted = true;
            assert!(matches!(current.state, CellState::Retracting | CellState::Ready));
            if current.state == CellState::Ready {
                assert!(current.frozen);
                return;
            }
        } else {
            assert!(!accepted);
            assert_eq!(current.generation, before.generation);
            assert_eq!(current.state, CellState::Published);
        }
        assert!(room_call::clock() < deadline, "Cell.Retract continuation timed out");
        room_call::yield_();
    }
}
#[track_caller]
fn delete(object: CellObject) {
    match object {
        CellObject::Slot(slot) => cell::delete(slot).unwrap(),
        CellObject::Pend(token) => {
            let deadline = room_call::clock() + 5_000_000_000;
            loop {
                match nest::release(token) {
                    Ok(()) => break,
                    Err(error) => {
                        assert_eq!(error.source, kernel_api::abi::call::nest::NestFail::Busy);
                        assert!(room_call::clock() < deadline, "mapping release stalled");
                        room_call::yield_();
                    }
                }
            }
        }
    }
}
fn attach(reference: SlotId, count: usize) -> (kernel_api::model::identity::PendId, VirtAddr) {
    let (mapping, address) = reserve(reference, count);
    for page in 0..count {
        assert_eq!(info(raw_memo(mapping, page)).state, CellState::Vacant);
        handoff(slot(reference, page), memo(mapping, page), false);
        assert_eq!(info(raw_memo(mapping, page)).state, CellState::Ready);
        publish(mapping, page);
    }
    (mapping, address)
}
fn read(address: VirtAddr, offset: usize) -> usize {
    unsafe { ((address.get() + offset) as *const usize).read_volatile() }
}
fn write(address: VirtAddr, offset: usize, value: usize) {
    unsafe { ((address.get() + offset) as *mut usize).write_volatile(value) }
}
#[unsafe(no_mangle)]
extern "C" fn run(args: usize, count: usize) -> ! {
    space::boot(args, count);
    space::run();
    #[cfg(feature = "context")]
    context::run();
    if cfg!(feature = "io-exit") {
        external::death(args, count);
    } else if cfg!(feature = "io") {
        external::run(args, count);
    }
    #[cfg(feature = "irq")]
    interrupt::run(args, count);
    license::run();
    observe::run(args, count);
    #[cfg(feature = "realtime")]
    realtime::run();
    let _ = writeln!(Console, "START cell-kernel");
    let a = allocate(2);
    let b = allocate(2);
    let (mapping, address) = attach(a, 2);
    write(address, 0, 0x12345678);
    write(address, 4096, 0xabcdef);
    let original = info(raw_memo(mapping, 0));
    handoff(slot(a, 0), slot(b, 1), false);
    assert_eq!(read(address, 0), 0x12345678);
    assert_eq!(info(raw_memo(mapping, 0)).page, original.page);
    let stale = memo(mapping, 0).to_bytes();
    retract(mapping, 0);
    assert!(cell::publish(ptr(&stale)).is_err());
    assert!(info(slot(b, 1)).frozen);
    // A Memo does not mint ownership: its declared source Slot is now empty.
    let denied = CellHandoff {
        remote: memo(mapping, 0),
        local: slot(a, 0),
    }
    .to_bytes();
    assert!(cell::pull(ptr(&denied)).is_err());
    handoff(slot(b, 1), slot(a, 0), true);
    // The current owning Slot must be named explicitly.
    handoff(slot(a, 0), slot(b, 0), true);
    assert_eq!(info(raw_memo(mapping, 0)).page, original.page);
    handoff(slot(b, 0), slot(a, 0), false);
    let (alias, _) = reserve(a, 2);
    handoff(memo(mapping, 0), memo(alias, 0), true);
    assert_eq!(info(raw_memo(alias, 0)).page, original.page);
    assert_eq!(info(raw_memo(alias, 0)).state, CellState::Ready);
    delete(raw_memo(alias, 0).object);
    publish(mapping, 0);
    assert_eq!(read(address, 0), 0x12345678);
    let _ = writeln!(
        Console,
        "PASS cell-kernel movement=1 memo=1 generation=1 freeze=1"
    );
    let saved = read(address, 0);
    retract(mapping, 0);
    let stale = memo(mapping, 0).to_bytes();
    handoff(slot(a, 0), slot(b, 0), false);
    delete(CellObject::Slot(b));
    let c = allocate(1);
    let (replacement, staging) = attach(c, 1);
    write(staging, 0, saved);
    retract(replacement, 0);
    handoff(slot(c, 0), slot(a, 0), false);
    handoff(slot(a, 0), memo(mapping, 0), false);
    assert_ne!(info(raw_memo(mapping, 0)).page, original.page);
    assert!(cell::publish(ptr(&stale)).is_err());
    // The staging Memo still freezes this Page: another Memo cannot bypass it.
    let blocked = memo(mapping, 0).to_bytes();
    assert!(cell::publish(ptr(&blocked)).is_err());
    delete(raw_memo(replacement, 0).object);
    publish(mapping, 0);
    assert_eq!(read(address, 0), saved);
    assert_eq!(read(address, 4096), 0xabcdef);
    delete(CellObject::Slot(c));
    assert!(nest::delete(mapping).is_err());
    retract(mapping, 0);
    retract(mapping, 1);
    delete(raw_memo(mapping, 0).object);
    delete(CellObject::Slot(a));
    let _ = writeln!(Console, "PASS cell-kernel swap=1 cleanup=1");
    levels::run();
    organization::run(args, count);
    lifecycle::entry::run();
    pend::run();
    sign::run();
    room::run();
    #[cfg(feature = "clock")]
    room::counter::run();
    let _ = writeln!(Console, "PASS room-kernel atomic-wait=1 wake=1 deadline=1");
    let _ = writeln!(
        Console,
        "PASS pend-kernel range=1 return=1 inspect=1 collect=1"
    );
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
