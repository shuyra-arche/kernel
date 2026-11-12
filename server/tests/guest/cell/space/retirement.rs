//! Grant return and full-reservation release have different cleanup boundaries.
use super::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::{
    abi::call::nest::NestFail,
    model::{
        cell::{CellInfo, CellObject, CellState},
        pend::NarrowRequest,
        unit::{ExitCause, ExitScope, UnitQuery, UnitTarget},
    },
};
use kernel_client::call::{cell, unit};

static RETIRED: AtomicUsize = AtomicUsize::new(0);

extern "C" fn stale_read() -> ! {
    let value = crate::read(VirtAddr::new(RETIRED.load(Ordering::Acquire)), 0);
    unit::exit(value as u64, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

fn faulted(address: VirtAddr) {
    RETIRED.store(address.get(), Ordering::Release);
    let task = crate::lifecycle::spawn(stale_read as *const () as usize);
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(crate::lifecycle::wait_task(task));
    let result = crate::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task)));
    assert!(
        matches!(result.exit, Some((ExitCause::Fault, _))),
        "retired span retained a PTE: {result:?}"
    );
    crate::lifecycle::delete(task);
}

fn observed(token: PendId, page: usize) -> Option<CellInfo> {
    let mut out = [0; CellInfo::WIDTH];
    cell::inspect(
        crate::ptr(&crate::raw_memo(token, page).to_bytes()),
        VirtAddr::new(out.as_mut_ptr() as usize),
    )
    .ok()?;
    CellInfo::from_bytes(&out)
}

fn grant(root: PendId, range: PendRange) -> PendId {
    accord(
        root,
        AccordTarget::Caller,
        Limits {
            scope: mapping(root, range),
            ..crate::pend::inspect(root).limits
        },
    )
}

fn branches() {
    let slots = crate::allocate(3);
    let (root, address) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        2,
        CellLevel::BASE,
        6,
    );
    let (unrelated, other_address) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        6,
    );
    let identity = id(root);
    for page in 0..2 {
        crate::handoff(crate::slot(slots, page), crate::memo(root, page), false);
    }
    crate::handoff(crate::slot(slots, 2), crate::memo(unrelated, 0), false);
    crate::publish(unrelated, 0);
    crate::write(other_address, 0, 0x778899);
    let child = grant(root, PendRange::Inherit);
    let sibling = grant(root, PendRange::Inherit);
    let partial = grant(
        root,
        PendRange::Pages {
            offset: 1,
            count: 1,
        },
    );
    assert_eq!(nest::release(partial).unwrap_err().source, NestFail::Denied);
    assert_eq!(id(root), identity);
    assert_eq!(
        crate::info(crate::raw_memo(root, 0)).state,
        CellState::Ready
    );
    crate::lifecycle::return_pend(partial);
    crate::publish(child, 0);
    crate::publish(sibling, 1);
    crate::write(address, 0, 0x112233);
    crate::write(address, 4096, 0x445566);
    crate::lifecycle::return_pend(child);
    assert!(observed(child, 0).is_none());
    assert_eq!(observed(root, 0).unwrap().state, CellState::Ready);
    assert_eq!(observed(root, 1).unwrap().state, CellState::Published);
    assert_eq!(crate::read(address, 4096), 0x445566);
    assert_eq!(id(root), identity);
    assert_eq!(
        nest::reserve(
            get(),
            Placement::exact(address, 4096).unwrap(),
            PageCount::new(2).unwrap(),
            CellLevel::BASE,
            6,
        )
        .unwrap_err()
        .source,
        NestFail::AlreadyMapped
    );
    crate::publish(root, 0);
    assert_eq!(crate::read(address, 0), 0x112233);
    let release = grant(root, PendRange::Inherit);
    crate::delete(CellObject::Pend(release));
    for token in [root, sibling, release] {
        assert!(observed(token, 0).is_none());
    }
    assert_eq!(
        crate::info(crate::raw_memo(unrelated, 0)).state,
        CellState::Published
    );
    assert_eq!(crate::read(other_address, 0), 0x778899);
    let (fresh, fresh_address) = reserve(
        get(),
        Placement::exact(address, 4096).unwrap(),
        2,
        CellLevel::BASE,
        6,
    );
    assert_eq!(fresh_address, address);
    assert_ne!(id(fresh), identity);
    for page in 0..2 {
        assert_eq!(
            crate::info(crate::raw_memo(fresh, page)).state,
            CellState::Vacant
        );
    }
    crate::lifecycle::return_pend(fresh);
    for token in [release, sibling, root] {
        crate::lifecycle::return_pend(token);
    }
    crate::retract(unrelated, 0);
    crate::lifecycle::return_pend(unrelated);
    crate::delete(CellObject::Slot(slots));
}

fn narrowed_root() {
    let slot = crate::allocate(3);
    let (root, address) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        3,
        CellLevel::BASE,
        6,
    );
    let old_id = id(root);
    for page in [0, 2] {
        crate::handoff(crate::slot(slot, page), crate::memo(root, page), false);
        crate::publish(root, page);
    }
    let request = NarrowRequest {
        token: root,
        limits: Limits {
            scope: mapping(
                root,
                PendRange::Pages {
                    offset: 1,
                    count: 1,
                },
            ),
            ..crate::pend::inspect(root).limits
        },
    }
    .to_bytes();
    pend::narrow(crate::ptr(&request)).unwrap();
    assert_eq!(
        crate::pend::inspect(root).limits.scope,
        mapping(
            root,
            PendRange::Pages {
                offset: 1,
                count: 1
            }
        )
    );
    assert_eq!(
        crate::info(crate::raw_memo(root, 0)).state,
        CellState::Vacant
    );
    crate::handoff(crate::slot(slot, 1), crate::memo(root, 0), false);
    crate::publish(root, 0);
    assert_eq!(nest::release(root).unwrap_err().source, NestFail::Denied);
    crate::lifecycle::return_pend(root);
    for page in 0..3 {
        faulted(address + page * 4096);
    }
    let (fresh, fresh_address) = reserve(
        get(),
        Placement::exact(address, 4096).unwrap(),
        3,
        CellLevel::BASE,
        6,
    );
    assert_eq!(fresh_address, address);
    assert_ne!(id(fresh), old_id);
    for page in 0..3 {
        let observation = crate::info(crate::raw_memo(fresh, page));
        assert_eq!(observation.state, CellState::Vacant);
        assert_eq!(observation.level, CellLevel::BASE);
    }
    assert!(observed(root, 0).is_none());
    crate::lifecycle::return_pend(fresh);
    crate::delete(CellObject::Slot(slot));
}

pub(super) fn run() {
    branches();
    narrowed_root();
    writeln!(crate::Console, "PASS nest-retirement child-return=1 full-release=1 partial-denied=1 sibling=1 narrowed-root=1 original-span=1").unwrap();
}
