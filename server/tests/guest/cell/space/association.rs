//! Per-position source identity, live qualification and sticky access ceilings.
use super::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::{
    abi::call::cell::CellFail,
    model::{
        cell::{CellAssociation, CellObject, CellState},
        modifiable::Modifiable,
        unit::{ExitCause, ExitScope, UnitQuery, UnitTarget},
    },
};
use kernel_client::call::{cell, share_view, unit};

static READ_ONLY: AtomicUsize = AtomicUsize::new(0);

extern "C" fn forbidden_write() -> ! {
    crate::write(VirtAddr::new(READ_ONLY.load(Ordering::Acquire)), 0, 0xbad);
    unit::exit(1, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

fn sources() {
    let a = crate::allocate(1);
    let b = crate::allocate(1);
    let c = crate::allocate(1);
    let empty = crate::allocate(1);
    let (mapping, address) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        2,
        CellLevel::BASE,
        6,
    );
    let identity = id(mapping);
    crate::handoff(crate::slot(a, 0), crate::memo(mapping, 0), false);
    crate::handoff(crate::slot(b, 0), crate::memo(mapping, 1), false);
    let old = crate::memo(mapping, 0);
    let sibling = crate::info(crate::raw_memo(mapping, 1));
    assert_ne!(crate::info(old).page, sibling.page);
    assert_eq!(crate::info(old).page, crate::info(crate::slot(a, 0)).page);
    assert_eq!(sibling.page, crate::info(crate::slot(b, 0)).page);
    crate::handoff(crate::slot(c, 0), old, false);
    let replacement = crate::info(crate::raw_memo(mapping, 0));
    assert_eq!(id(mapping), identity);
    assert_eq!(replacement.generation, old.generation + 1);
    assert_eq!(replacement.page, crate::info(crate::slot(c, 0)).page);
    assert_eq!(crate::info(crate::raw_memo(mapping, 1)), sibling);
    assert!(cell::publish(crate::ptr(&old.to_bytes())).is_err());
    crate::publish(mapping, 0);
    crate::publish(mapping, 1);
    crate::write(address, 0, 0x314159);
    crate::write(address, 4096, 0x271828);

    let published = crate::info(crate::raw_memo(mapping, 0));
    crate::handoff(crate::slot(c, 0), crate::slot(empty, 0), false);
    assert_eq!(crate::info(crate::raw_memo(mapping, 0)), published);
    assert_eq!(crate::read(address, 0), 0x314159);
    assert_eq!(crate::read(address, 4096), 0x271828);
    crate::retract(mapping, 0);
    let unqualified = crate::memo(mapping, 0);
    assert!(cell::publish(crate::ptr(&unqualified.to_bytes())).is_err());
    assert_eq!(crate::info(unqualified).page, published.page);
    crate::handoff(crate::slot(a, 0), crate::slot(c, 0), false);
    assert_ne!(crate::info(crate::slot(c, 0)).page, published.page);
    assert!(cell::publish(crate::ptr(&unqualified.to_bytes())).is_err());
    assert_eq!(crate::info(unqualified).page, published.page);
    assert_eq!(crate::info(unqualified).generation, unqualified.generation);
    crate::handoff(crate::slot(c, 0), unqualified, false);
    assert_ne!(
        crate::info(crate::raw_memo(mapping, 0)).page,
        published.page
    );
    assert!(cell::publish(crate::ptr(&unqualified.to_bytes())).is_err());
    crate::publish(mapping, 0);
    assert_eq!(crate::read(address, 0), 0);
    assert_eq!(crate::read(address, 4096), 0x271828);
    assert_eq!(id(mapping), identity);
    for page in 0..2 {
        crate::retract(mapping, page);
    }
    crate::delete(CellObject::Pend(mapping));
    crate::lifecycle::return_pend(mapping);
    for slot in [a, b, c, empty] {
        crate::delete(CellObject::Slot(slot));
    }
}

fn protect_denied(mapping: PendId) {
    let position = crate::memo(mapping, 0);
    let before = crate::info(position);
    assert!(matches!(
        cell::protect(crate::ptr(&position.to_bytes()), 6)
            .unwrap_err()
            .source,
        CellFail::Denied | CellFail::WidenDenied
    ));
    assert_eq!(crate::info(position), before);
}

fn readonly(source_access: u64, source_modifiable: Modifiable, supplying_modifiable: Modifiable) {
    let slot = crate::allocate(1);
    let (staging, address) = crate::attach(slot, 1);
    crate::write(address, 0, 0xabcdef);
    crate::retract(staging, 0);
    crate::delete(CellObject::Pend(staging));
    crate::lifecycle::return_pend(staging);
    let share = share_view::create(crate::license::get(), slot, source_modifiable).unwrap();
    let (source, _) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        source_access,
    );
    let invalid = CellAssociation {
        source: kernel_api::model::cell::CellPosition {
            generation: 1,
            ..crate::raw_memo(share, 0)
        },
        target: crate::memo(source, 0),
    }
    .to_bytes();
    assert!(cell::insert(crate::ptr(&invalid)).is_err());
    crate::handoff(crate::raw_memo(share, 0), crate::memo(source, 0), false);
    let supplying = accord(
        source,
        AccordTarget::Caller,
        Limits {
            operations: Operations::Nest(NestRights::INSPECT | NestRights::INSERT),
            modifiable: supplying_modifiable,
            distributable: Distributable::No,
            scope: mapping(source, PendRange::Inherit),
        },
    );
    let (first, _) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        6,
    );
    let (second, address) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        6,
    );
    for token in [first, second] {
        assert_eq!(
            crate::pend::inspect(token).limits.modifiable,
            Modifiable::Yes
        );
    }
    let invalid = CellAssociation {
        source: crate::raw_memo(supplying, 0),
        target: crate::memo(first, 0),
    }
    .to_bytes();
    assert!(cell::insert(crate::ptr(&invalid)).is_err());
    crate::handoff(crate::memo(supplying, 0), crate::memo(first, 0), false);
    let before = crate::info(crate::raw_memo(first, 0));
    assert_eq!(
        cell::publish(crate::ptr(&crate::memo(first, 0).to_bytes()))
            .unwrap_err()
            .source,
        CellFail::Denied
    );
    assert_eq!(crate::info(crate::raw_memo(first, 0)), before);
    cell::protect(crate::ptr(&crate::memo(first, 0).to_bytes()), 2).unwrap();
    crate::handoff(crate::memo(first, 0), crate::memo(second, 0), false);
    let before = crate::info(crate::raw_memo(second, 0));
    assert_eq!(
        cell::publish(crate::ptr(&crate::memo(second, 0).to_bytes()))
            .unwrap_err()
            .source,
        CellFail::Denied
    );
    assert_eq!(crate::info(crate::raw_memo(second, 0)), before);
    cell::protect(crate::ptr(&crate::memo(second, 0).to_bytes()), 2).unwrap();
    protect_denied(first);
    protect_denied(second);
    crate::publish(second, 0);
    assert_eq!(crate::read(address, 0), 0xabcdef);
    crate::lifecycle::return_pend(supplying);
    crate::lifecycle::return_pend(source);
    crate::lifecycle::return_pend(first);
    assert_eq!(
        crate::info(crate::raw_memo(second, 0)).state,
        CellState::Published
    );
    assert_eq!(crate::read(address, 0), 0xabcdef);
    protect_denied(second);

    READ_ONLY.store(address.get(), Ordering::Release);
    let task = crate::lifecycle::spawn(forbidden_write as *const () as usize);
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(crate::lifecycle::wait_task(task));
    let exited = crate::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task)));
    assert!(
        matches!(exited.exit, Some((ExitCause::Fault, _))),
        "{exited:?}"
    );
    crate::lifecycle::delete(task);
    assert_eq!(crate::read(address, 0), 0xabcdef);
    crate::retract(second, 0);
    crate::delete(CellObject::Pend(second));
    crate::lifecycle::return_pend(second);
    crate::lifecycle::return_pend(share);
    crate::delete(CellObject::Slot(slot));
}

pub(super) fn run() {
    sources();
    for (access, source, supplying) in [
        (6, Modifiable::Yes, Modifiable::No),
        (2, Modifiable::Yes, Modifiable::Yes),
        (6, Modifiable::No, Modifiable::Yes),
    ] {
        readonly(access, source, supplying);
    }
    writeln!(crate::Console, "PASS nest-association unrelated=2 replacement=1 stale=1 moved-source=1 readonly-copies=2 write-fault=1 alias-return=1 ceilings=3").unwrap();
}
