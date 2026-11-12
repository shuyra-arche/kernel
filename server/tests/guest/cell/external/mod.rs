use kernel_api::{
    address::{PageCount, VirtAddr},
    boot::{
        args,
        ledger::{Entries, Name, Page, ResourceKind},
    },
    model::{
        cell::*,
        identity::{Mark, PendId},
        modifiable::Modifiable,
        nest::{NestInfo, NestKind, NestPhase},
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, NestRights, NestScope, Operations,
            PendScope,
        },
        sign::SignSource,
        wait::Wait,
    },
};
use kernel_client::call::{cell, nest, pend, room, sign};
mod block;
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
        ledger
            .find(Name::Page(Page::Region(0x40000000)))
            .unwrap()
            .handle(),
    );
    let entry = ledger
        .iter()
        .find(|e| matches!(e.name(), Name::Nest { .. }))
        .unwrap();
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
        operations: Operations::Nest(NestRights::INSPECT | NestRights::RESERVE),
        modifiable: Modifiable::Yes,
        distributable: Distributable::No,
        scope: PendScope::Nest(NestScope::Whole),
    }
}
fn branch(root: PendId) -> PendId {
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits: limits(),
    }
    .to_bytes();
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
    CellPosition {
        object: CellObject::Pend(token),
        ..pos
    }
}
fn observed(pos: CellPosition) -> Option<CellInfo> {
    let mut bytes = [0; CellInfo::WIDTH];
    cell::inspect(
        super::ptr(&pos.to_bytes()),
        VirtAddr::new(bytes.as_mut_ptr() as usize),
    )
    .ok()?;
    CellInfo::from_bytes(&bytes)
}
fn current(mut pos: CellPosition) -> CellPosition {
    pos.generation = super::info(pos).generation;
    pos
}
fn create(token: PendId, slot: SlotId, address: usize, writable: bool) -> CellPosition {
    let pos = reserve(token, slot, address, writable).unwrap();
    super::handoff(super::slot(slot, 0), pos, false);
    current(pos)
}

fn reserve(
    token: PendId,
    slot: SlotId,
    address: usize,
    writable: bool,
) -> Result<CellPosition, kernel_api::abi::call::nest::NestFail> {
    let level = super::info(super::slot(slot, 0)).level;
    let (mapping, reserved) = nest::reserve(
        token,
        Placement::exact(VirtAddr::new(address), level.bytes().unwrap()).unwrap(),
        PageCount::new(level.pages().unwrap()).unwrap(),
        level,
        if writable { 6 } else { 2 },
    )
    .map_err(|error| error.source)?;
    assert_eq!(reserved.get(), address);
    let position = super::memo(mapping, 0);
    let observation = super::info(position);
    assert_eq!(observation.state, CellState::Vacant);
    assert_eq!(observation.level, level);
    Ok(position)
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
        assert!(
            room::clock() < until,
            "Nest close deadline {:?}",
            context(token)
        );
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
    let CellObject::Pend(token) = pos.object else {
        panic!("external position")
    };
    let mut bytes = [0; kernel_api::model::pend::PendInfo::SIZE];
    pend::inspect(token, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap();
    let observation = kernel_api::model::pend::PendInfo::from_bytes(&bytes).unwrap();
    let PendScope::Nest(NestScope::Mapping { id: mapping, .. }) = observation.limits.scope else {
        panic!("mapping-scoped Nest grant expected")
    };
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
