#![no_std]
#![no_main]
mod lifecycle;
mod pend;
mod sign;
use core::fmt::Write;
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::{
            AcquireSource, Allocation, Attachment, FlatReference, MappingControl, MappingId,
            MappingSelection, MappingTarget, Placement, Position, ReferenceRange,
        },
        identity::TeamId,
        unit::ControlId,
    },
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
fn allocate(count: usize) -> FlatReference {
    cell::acquire(
        AcquireSource::Allocation(Allocation::new(count * 4096, 4096).unwrap()),
        ControlId::NONE,
    )
    .unwrap()
}
fn attach(reference: FlatReference, count: usize) -> (MappingId, VirtAddr) {
    let spec = Attachment {
        range: ReferenceRange {
            reference,
            offset: 0,
            count,
        },
        placement: Placement::lowest(4096).unwrap(),
        access: 6,
        control: MappingControl::Caller,
        change: 0,
    }
    .to_bytes();
    cell::attach(
        MappingTarget::Cpu(TeamId::new(0)),
        VirtAddr::new(spec.as_ptr() as usize),
    )
    .unwrap()
}
fn push(reference: FlatReference, page: usize, target: Position, change: usize) {
    let target = target.to_bytes();
    cell::push(
        reference,
        page,
        VirtAddr::new(target.as_ptr() as usize),
        change,
    )
    .unwrap();
}
fn pull(source: Position, target: FlatReference, page: usize, change: usize) {
    let source = source.to_bytes();
    cell::pull(
        VirtAddr::new(source.as_ptr() as usize),
        target,
        page,
        change,
    )
    .unwrap();
}
fn memo(mapping: MappingId, page: usize) -> Position {
    Position::Memo {
        team: TeamId::new(0),
        mapping,
        page,
    }
}
fn read(address: VirtAddr, offset: usize) -> usize {
    unsafe { ((address.get() + offset) as *const usize).read_volatile() }
}
fn write(address: VirtAddr, offset: usize, value: usize) {
    unsafe { ((address.get() + offset) as *mut usize).write_volatile(value) }
}
#[unsafe(no_mangle)]
extern "C" fn run() -> ! {
    let _ = writeln!(Console, "START cell-kernel");
    let a = allocate(2);
    let b = allocate(2);
    let (mapping, address) = attach(a, 2);
    write(address, 0, 0x12345678);
    write(address, 4096, 0xabcdef);
    let receipt = cell::begin(a, 0).unwrap();
    push(
        a,
        0,
        Position::Slot {
            reference: b,
            page: 1,
        },
        receipt,
    );
    assert_eq!(read(address, 0), 0x12345678); // retained Memo survives Slot movement
    assert!(cell::begin(b, 1).is_err());
    let wrong = memo(mapping, 1).to_bytes();
    assert!(
        cell::pull(
            VirtAddr::new(wrong.as_ptr() as usize),
            FlatReference::NONE,
            0,
            receipt
        )
        .is_err()
    );
    pull(memo(mapping, 0), FlatReference::NONE, 0, receipt);
    pull(
        Position::Slot {
            reference: b,
            page: 1,
        },
        a,
        0,
        receipt,
    );
    push(a, 0, memo(mapping, 0), receipt);
    cell::end(receipt).unwrap();
    assert!(cell::end(receipt).is_err());
    assert_eq!(read(address, 0), 0x12345678);
    assert_eq!(read(address, 4096), 0xabcdef);
    let _ = writeln!(Console, "PASS cell-kernel movement=1 memo=1 receipt=1");
    // Swap protocol: save bytes, withdraw, move out and release the old backing.
    let saved = read(address, 0);
    let receipt = cell::begin(a, 0).unwrap();
    pull(memo(mapping, 0), FlatReference::NONE, 0, receipt);
    push(
        a,
        0,
        Position::Slot {
            reference: b,
            page: 0,
        },
        receipt,
    );
    cell::end(receipt).unwrap();
    cell::release(b).unwrap();
    // Restore into a different Page and supply the original stable Memo.
    let c = allocate(1);
    let (replacement, staging) = attach(c, 1);
    write(staging, 0, saved);
    let receipt = cell::begin(c, 0).unwrap();
    pull(memo(replacement, 0), FlatReference::NONE, 0, receipt);
    push(
        c,
        0,
        Position::Slot {
            reference: a,
            page: 0,
        },
        receipt,
    );
    push(a, 0, memo(mapping, 0), receipt);
    cell::end(receipt).unwrap();
    assert_eq!(read(address, 0), saved);
    assert_eq!(read(address, 4096), 0xabcdef);
    cell::detach(
        MappingTarget::Cpu(TeamId::new(0)),
        replacement,
        MappingSelection::Whole,
    )
    .unwrap();
    cell::release(c).unwrap();
    cell::detach(
        MappingTarget::Cpu(TeamId::new(0)),
        mapping,
        MappingSelection::Whole,
    )
    .unwrap();
    cell::release(a).unwrap();
    let _ = writeln!(Console, "PASS cell-kernel swap=1 cleanup=1");
    pend::run();
    sign::run();
    let _ = writeln!(
        Console,
        "PASS pend-kernel range=1 return=1 inspect=1 collect=1"
    );
    unit::exit(0, 0, VirtAddr::new(0), 0)
}
