//! Scope identities and attenuation through real Pend/Nest calls.
use super::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::{
    abi::call::{nest::NestFail, pend::PendFail},
    model::{
        cell::{CellInfo, CellState},
        modifiable::Modifiable,
        pend::NarrowRequest,
        unit::{ExitScope, UnitTarget},
    },
};
use kernel_client::call::{cell, unit};

static FOREIGN: AtomicUsize = AtomicUsize::new(0);

extern "C" fn empty_ledger() -> ! {
    let mut out = [0; PendInfo::WORDS];
    assert_eq!(
        pend::collect(PendId::NONE, VirtAddr::new(out.as_mut_ptr() as usize), 1).unwrap(),
        0
    );
    assert!(find().is_none());
    assert_eq!(
        nest::reserve(
            PendId::mint(FOREIGN.load(Ordering::Acquire)),
            Placement::lowest(4096).unwrap(),
            PageCount::new(1).unwrap(),
            CellLevel::BASE,
            2,
        )
        .unwrap_err()
        .source,
        NestFail::Denied
    );
    unit::exit(10, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

fn denied(src: PendId, limits: Limits) {
    assert!(limits.valid());
    let before = crate::pend::inspect(src).limits;
    let request = AccordRequest {
        src,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits,
    }
    .to_bytes();
    assert_eq!(
        pend::accord(crate::ptr(&request)).unwrap_err().source,
        PendFail::Denied
    );
    let request = NarrowRequest { token: src, limits }.to_bytes();
    assert_eq!(
        pend::narrow(crate::ptr(&request)).unwrap_err().source,
        PendFail::Denied
    );
    assert_eq!(crate::pend::inspect(src).limits, before);
}

pub(super) fn run() {
    let whole = get();
    let parent = crate::pend::inspect(whole);
    // No Cell.Create, Slot, Flat, Share or source-geometry query precedes Reserve.
    let (vacant, address) = reserve(
        whole,
        Placement::lowest(1 << 21).unwrap(),
        512,
        CellLevel::new(1).unwrap(),
        2,
    );
    let observed = crate::info(crate::raw_memo(vacant, 0));
    assert_eq!(observed.state, CellState::Vacant);
    assert_eq!(observed.level.get(), 1);
    assert_eq!(observed.page, 0);
    assert_eq!(observed.address, address);
    assert!(!observed.external);
    let first_id = id(vacant);
    let original = crate::pend::inspect(vacant);
    assert_eq!(original.target, parent.target);
    assert_eq!(
        original.limits.scope,
        mapping(
            vacant,
            PendRange::Pages {
                offset: 0,
                count: 512
            }
        )
    );
    let mut out = [0; CellInfo::WIDTH];
    assert!(
        cell::inspect(
            crate::ptr(&crate::raw_memo(whole, 0).to_bytes()),
            VirtAddr::new(out.as_mut_ptr() as usize),
        )
        .is_err()
    );
    assert_eq!(nest::release(whole).unwrap_err().source, NestFail::Denied);
    assert_eq!(nest::delete(vacant).unwrap_err().source, NestFail::Denied);
    assert_eq!(
        nest::reserve(
            vacant,
            Placement::lowest(4096).unwrap(),
            PageCount::new(1).unwrap(),
            CellLevel::BASE,
            2,
        )
        .unwrap_err()
        .source,
        NestFail::Denied
    );

    let inspect = Limits {
        operations: Operations::Nest(NestRights::INSPECT),
        modifiable: Modifiable::No,
        distributable: Distributable::No,
        scope: mapping(vacant, PendRange::Inherit),
    };
    denied(whole, inspect);
    denied(
        vacant,
        Limits {
            scope: PendScope::Nest(NestScope::Whole),
            ..inspect
        },
    );
    let (other, _) = reserve(
        whole,
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        2,
    );
    assert_ne!(id(other), first_id);
    denied(
        vacant,
        Limits {
            scope: mapping(other, PendRange::Inherit),
            ..inspect
        },
    );
    let restricted = accord(vacant, AccordTarget::Caller, inspect);
    denied(
        restricted,
        Limits {
            modifiable: Modifiable::Yes,
            ..inspect
        },
    );
    denied(
        restricted,
        Limits {
            distributable: Distributable::Yes,
            ..inspect
        },
    );
    let request = AccordRequest {
        src: restricted,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits: inspect,
    }
    .to_bytes();
    assert_eq!(
        pend::accord(crate::ptr(&request)).unwrap_err().source,
        PendFail::Denied
    );
    crate::lifecycle::return_pend(restricted);
    for token in [vacant, other] {
        crate::delete(crate::raw_memo(token, 0).object);
        crate::lifecycle::return_pend(token);
    }

    let limited = accord(
        whole,
        AccordTarget::Caller,
        Limits {
            operations: Operations::Nest(NestRights::INSPECT | NestRights::RESERVE),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: PendScope::Nest(NestScope::Whole),
        },
    );
    assert_eq!(
        nest::reserve(
            limited,
            Placement::lowest(4096).unwrap(),
            PageCount::new(1).unwrap(),
            CellLevel::BASE,
            6,
        )
        .unwrap_err()
        .source,
        NestFail::Denied
    );
    let (root, _) = reserve(
        limited,
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        2,
    );
    let limits = crate::pend::inspect(root).limits;
    assert_eq!(limits.modifiable, Modifiable::No);
    assert_eq!(limits.distributable, Distributable::No);
    assert_eq!(limits.operations, Operations::Nest(NestRights::mapping()));
    let request = NarrowRequest {
        token: limited,
        limits: Limits {
            operations: Operations::Nest(NestRights::INSPECT),
            ..crate::pend::inspect(limited).limits
        },
    }
    .to_bytes();
    assert_eq!(
        pend::narrow(crate::ptr(&request)).unwrap_err().source,
        PendFail::Busy
    );
    crate::lifecycle::return_pend(root);
    pend::narrow(crate::ptr(&request)).unwrap();
    crate::lifecycle::return_pend(limited);

    FOREIGN.store(whole.get(), Ordering::Release);
    let task = crate::lifecycle::spawn(empty_ledger as *const () as usize);
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(crate::lifecycle::wait_task(task));
    assert_eq!(crate::lifecycle::result(task), (1, 10));
    crate::lifecycle::delete(task);
    writeln!(
        crate::Console,
        "PASS nest-scope sourceless=1 level=1 identity=1 attenuation=1 licensed=1 empty-ledger=1"
    )
    .unwrap();
}
