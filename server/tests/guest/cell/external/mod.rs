use kernel_api::{
    address::VirtAddr,
    boot::{
        args,
        ledger::{Entries, Name, Page, ResourceKind},
    },
    model::{
        cell::*,
        identity::{Mark, PendId},
        modifiable::Modifiable,
        nest::{NestInfo, NestKind, NestPhase},
        pend::{AccordRequest, AccordTarget, Distributable, Limits, NestRights, Operations, PendRange},
        sign::SignSource,
        wait::Wait,
    },
};
use kernel_client::call::{cell, nest, pend, room, sign};
mod exit;
mod levels;
mod publication;
pub(super) use exit::death;
pub(super) use publication::run;
const ADDRESS: usize = 0x400000;
fn roots(at: usize, count: usize) -> (PendId, PendId) {
    assert_eq!(count, args::LEN);
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let ledger = Entries::new(unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    })
    .unwrap();
    let bar = PendId::mint(
        ledger.find(Name::Page(Page::Region(0x40000000))).unwrap().handle(),
    );
    let entry = ledger.iter().find(|e| matches!(e.name(), Name::Nest { .. })).unwrap();
    assert_eq!(entry.kind(), ResourceKind::Nest);
    assert!(matches!(entry.name(), Name::Nest { requester: 8, .. }));
    let root = PendId::mint(entry.handle());
    assert_eq!(context(root).kind, NestKind::External);
    assert_eq!(context(root).phase, NestPhase::Bound);
    (bar, root)
}
fn context(token: PendId) -> NestInfo {
    let mut bytes = [0; NestInfo::WIDTH];
    nest::inspect(token, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
    NestInfo::from_bytes(&bytes).unwrap()
}
fn limits() -> Limits {
    Limits {
        operations: Operations::Nest(NestRights::all() & !(NestRights::DELETE | NestRights::ATTACH)),
        modifiable: Modifiable::Yes,
        distributable: Distributable::No,
        range: PendRange::Inherit,
    }
}
fn branch(root: PendId) -> PendId {
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits: limits(),
    }.to_bytes();
    pend::accord(super::ptr(&bytes)).unwrap()
}
fn returned(token: PendId) {
    let until = room::clock() + 2_000_000_000;
    loop {
        match pend::return_(token) {
            Ok(()) => break,
            Err(error) => {
                assert_eq!(error.source, kernel_api::abi::call::pend::PendFail::Busy);
                assert!(room::clock() < until, "Pend return stalled");
                room::yield_();
            }
        }
    }
}
fn alias(pos: CellPosition, token: PendId) -> CellPosition {
    let CellObject::Memo { mapping, .. } = pos.object else { panic!("Memo expected") };
    CellPosition {
        object: CellObject::Memo { target: MappingTarget::Io(token), mapping },
        ..pos
    }
}
fn observed(pos: CellPosition) -> Option<CellInfo> {
    let mut bytes = [0; CellInfo::WIDTH];
    cell::inspect(super::ptr(&pos.to_bytes()), VirtAddr::new(bytes.as_mut_ptr() as usize)).ok()?;
    CellInfo::from_bytes(&bytes)
}
fn current(mut pos: CellPosition) -> CellPosition {
    pos.generation = super::info(pos).generation;
    pos
}
fn create(token: PendId, slot: SlotId, address: usize, writable: bool) -> CellPosition {
    let bytes = CellCreate::Memo {
        source: CellSource::Slot(ReferenceRange {
            reference: slot,
            offset: 0,
            count: super::info(super::slot(slot, 0)).level.pages().unwrap(),
        }),
        destination: MemoDestination::External {
            authority: token,
            address: VirtAddr::new(address),
            access: if writable { Modifiable::Yes } else { Modifiable::No },
        },
    }.to_bytes();
    let CreatedCell::Memo { mapping, .. } = cell::create(PendId::NONE, super::ptr(&bytes)).unwrap()
    else { panic!("Memo expected") };
    let pos = CellPosition {
        object: CellObject::Memo { target: MappingTarget::Io(token), mapping },
        page: 0,
        generation: 1,
    };
    super::handoff(super::slot(slot, 0), pos, false);
    current(pos)
}
fn wait(pos: CellPosition, state: CellState) {
    let until = room::clock() + 2_000_000_000;
    while super::info(pos).state != state {
        assert!(room::clock() < until, "Memo deadline {state:?}");
        room::yield_();
    }
}
fn publish(pos: CellPosition) {
    cell::publish(super::ptr(&current(pos).to_bytes())).unwrap();
    wait(pos, CellState::Published);
}
fn retract(pos: CellPosition) {
    cell::retract(super::ptr(&current(pos).to_bytes())).unwrap();
    wait(pos, CellState::Ready);
}
fn closed(token: PendId) {
    let until = room::clock() + 2_000_000_000;
    while context(token).phase != NestPhase::Closed {
        assert!(room::clock() < until, "Nest close deadline {:?}", context(token));
        room::yield_();
    }
}
fn read(base: VirtAddr, offset: usize) -> u32 {
    unsafe {
        core::arch::asm!("fence iorw,iorw", options(nostack));
        ((base.get() + offset) as *const u32).read_volatile()
    }
}
fn write(base: VirtAddr, offset: usize, value: u64) {
    unsafe {
        core::arch::asm!("fence iorw,iorw", options(nostack));
        ((base.get() + offset) as *mut u64).write_volatile(value);
        core::arch::asm!("fence iorw,iorw", options(nostack));
    }
}
fn start(bar: VirtAddr, address: usize, writing: bool) {
    write(bar, 0x80, if writing { 0x40000 } else { address as u64 });
    write(bar, 0x88, if writing { address as u64 } else { 0x40000 });
    write(bar, 0x90, 64);
    write(bar, 0x98, if writing { 3 } else { 1 });
}
fn complete(bar: VirtAddr) {
    let until = room::clock() + 2_000_000_000;
    while read(bar, 0x98) & 1 != 0 {
        assert!(room::clock() < until, "EDU deadline");
        room::sleep_until(room::clock() + 1_000_000).unwrap();
    }
}
fn transfer(bar: VirtAddr, address: usize, writing: bool) {
    start(bar, address, writing);
    complete(bar);
}
fn source(pos: CellPosition, published: bool) -> SignSource {
    let CellObject::Memo { target: MappingTarget::Io(token), mapping } = pos.object
    else { panic!("external position") };
    if published {
        SignSource::MappingPublished { token, mapping }
    } else {
        SignSource::MappingRetired { token, mapping }
    }
}
fn event(token: PendId, source: SignSource) {
    assert_eq!(
        sign::listen(token, Wait::At(room::clock() + 2_000_000_000)).unwrap(),
        kernel_api::abi::call::sign::SignReply::Source { source, fail: None }
    );
}
