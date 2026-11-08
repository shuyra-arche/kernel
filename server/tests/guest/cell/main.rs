#![no_std]
#![no_main]
#[cfg(feature = "context")]
mod context;
mod external;
mod license;
mod lifecycle;
mod pend;
mod room;
mod sign;
use core::fmt::Write;
use kernel_api::{
    address::VirtAddr,
    model::{cell::*, identity::TeamId},
};
use kernel_client::call::{cell, debug, unit};
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
        while !bytes.is_empty() {
            let size = bytes.len().min(256);
            let n = debug::put(VirtAddr::new(bytes.as_ptr() as usize), size)
                .map_err(|_| core::fmt::Error)?;
            if n == 0 {
                return Err(core::fmt::Error);
            }
            bytes = &bytes[n..];
        }
        Ok(())
    }
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL cell-kernel {info}");
    unit::exit(1, 0, VirtAddr::new(0), 0)
}
fn ptr(bytes: &[u8]) -> VirtAddr {
    VirtAddr::new(bytes.as_ptr() as usize)
}
fn allocate(count: usize) -> SlotId {
    let spec = CellCreate::Slot(Allocation::new(count * 4096, 4096).unwrap()).to_bytes();
    let made = cell::create(crate::license::get(), ptr(&spec)).unwrap();
    let CreatedCell::Slot(slot) = made else {
        panic!("expected Slot")
    };
    slot
}
fn reserve(reference: SlotId, count: usize) -> (MappingId, VirtAddr) {
    let spec = CellCreate::Memo {
        source: CellSource::Slot(ReferenceRange {
            reference,
            offset: 0,
            count,
        }),
        destination: MemoDestination::Internal {
            team: TeamId::new(0),
            placement: Placement::lowest(4096).unwrap(),
            access: 6,
            control: MappingControl::Caller,
        },
    }
    .to_bytes();
    let made =
        cell::create(kernel_api::model::identity::PendId::NONE, ptr(&spec)).unwrap_or_else(|e| {
            panic!(
                "Memo create source={} count={} {e:?}",
                reference.get(),
                count
            )
        });
    let CreatedCell::Memo { mapping, address } = made else {
        panic!("expected Memo")
    };
    (mapping, address)
}
fn slot(reference: SlotId, page: usize) -> CellPosition {
    CellPosition {
        object: CellObject::Slot(reference),
        page,
        generation: 0,
    }
}
fn raw_memo(mapping: MappingId, page: usize) -> CellPosition {
    CellPosition {
        object: CellObject::Memo {
            target: MappingTarget::Cpu(TeamId::new(0)),
            mapping,
        },
        page,
        generation: 0,
    }
}
fn info(pos: CellPosition) -> CellInfo {
    let position = pos.to_bytes();
    let mut bytes = [0; CellInfo::WIDTH];
    cell::inspect(ptr(&position), VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
    CellInfo::from_bytes(&bytes).unwrap()
}
fn memo(mapping: MappingId, page: usize) -> CellPosition {
    let mut pos = raw_memo(mapping, page);
    pos.generation = info(pos).generation;
    pos
}
fn handoff(source: CellPosition, target: CellPosition, pull: bool) {
    let spec = CellHandoff { source, target }.to_bytes();
    if pull {
        cell::pull(ptr(&spec))
    } else {
        cell::push(ptr(&spec))
    }
    .unwrap();
}
fn publish(mapping: MappingId, page: usize) {
    let pos = memo(mapping, page).to_bytes();
    cell::publish(ptr(&pos)).unwrap();
    assert_eq!(info(raw_memo(mapping, page)).state, CellState::Published);
}
fn retract(mapping: MappingId, page: usize) {
    let pos = memo(mapping, page).to_bytes();
    cell::retract(ptr(&pos)).unwrap();
    let info = info(raw_memo(mapping, page));
    assert_eq!(info.state, CellState::Ready);
    assert!(info.frozen);
}
fn delete(object: CellObject) {
    cell::delete(ptr(&object.to_bytes())).unwrap();
}
fn attach(reference: SlotId, count: usize) -> (MappingId, VirtAddr) {
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
    #[cfg(feature = "context")]
    context::run();
    if cfg!(feature = "io-exit") {
        external::death(args, count);
    } else if cfg!(feature = "io") {
        external::run(args, count);
    }
    license::run();
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
        source: memo(mapping, 0),
        target: slot(a, 0),
    }
    .to_bytes();
    assert!(cell::pull(ptr(&denied)).is_err());
    handoff(slot(b, 1), slot(a, 0), true);
    // Memo -> Slot moves its actual controlled Slot, leaving the Memo unchanged.
    handoff(memo(mapping, 0), slot(b, 0), true);
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
    assert!(cell::delete(ptr(&raw_memo(mapping, 0).object.to_bytes())).is_err());
    retract(mapping, 0);
    retract(mapping, 1);
    delete(raw_memo(mapping, 0).object);
    delete(CellObject::Slot(a));
    let _ = writeln!(Console, "PASS cell-kernel swap=1 cleanup=1");
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
    unit::exit(0, 0, VirtAddr::new(0), 0)
}
