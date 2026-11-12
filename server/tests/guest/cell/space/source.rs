//! A deleted source handle cannot hide foreign CPU uses from Task retirement.
use super::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::{
    abi::call::nest::NestFail,
    model::{
        cell::{CellInfo, CellObject, CellState},
        modifiable::Modifiable,
        pend::{NarrowRequest, PendStatus},
        unit::{ExitCause, ExitScope, UnitQuery, UnitState, UnitTarget},
    },
};
use kernel_client::call::{cell, room, unit};

static CONTROLLER: AtomicUsize = AtomicUsize::new(0);
static PUBLISHER: AtomicUsize = AtomicUsize::new(0);
static ALIAS: AtomicUsize = AtomicUsize::new(0);
static OBSERVER: AtomicUsize = AtomicUsize::new(0);
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static COPIED: AtomicUsize = AtomicUsize::new(0);
static DELETED: AtomicUsize = AtomicUsize::new(0);
static EXIT: AtomicUsize = AtomicUsize::new(0);
static CLEANUP: AtomicUsize = AtomicUsize::new(0);

fn until(flag: &AtomicUsize) -> usize {
    let deadline = room::clock() + 5_000_000_000;
    loop {
        let value = flag.load(Ordering::Acquire);
        if value != 0 {
            return value;
        }
        assert!(room::clock() < deadline, "source fixture handshake stalled");
        room::yield_();
    }
}

extern "C" fn owner() -> ! {
    let slot = crate::allocate(1);
    let (staging, address) = crate::attach(slot, 1);
    crate::write(address, 0, 0x13579bdf);
    crate::retract(staging, 0);
    crate::delete(CellObject::Pend(staging));
    crate::lifecycle::return_pend(staging);
    let (source, _) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        6,
    );
    crate::handoff(crate::slot(slot, 0), crate::memo(source, 0), false);
    let alias = accord(
        source,
        AccordTarget::Task(TaskId::new(PUBLISHER.load(Ordering::Acquire))),
        Limits {
            operations: Operations::Nest(NestRights::INSPECT | NestRights::INSERT),
            modifiable: Modifiable::Yes,
            distributable: Distributable::No,
            scope: mapping(source, PendRange::Inherit),
        },
    );
    ALIAS.store(alias.get(), Ordering::Release);
    until(&COPIED);
    let request = NarrowRequest {
        token: source,
        limits: Limits {
            operations: Operations::Nest(NestRights::INSPECT),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: mapping(source, PendRange::Inherit),
        },
    }
    .to_bytes();
    pend::narrow(crate::ptr(&request)).unwrap();
    crate::delete(CellObject::Slot(slot));
    let mut out = [0; CellInfo::WIDTH];
    assert!(
        cell::inspect(
            crate::ptr(&crate::slot(slot, 0).to_bytes()),
            VirtAddr::new(out.as_mut_ptr() as usize),
        )
        .is_err()
    );
    DELETED.store(1, Ordering::Release);
    until(&EXIT);
    unit::exit(77, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

extern "C" fn publisher() -> ! {
    let alias = PendId::mint(until(&ALIAS));
    let own = crate::allocate(1);
    let (root, address) = reserve(
        get(),
        Placement::lowest(4096).unwrap(),
        2,
        CellLevel::BASE,
        6,
    );
    crate::handoff(crate::memo(alias, 0), crate::memo(root, 0), false);
    crate::handoff(crate::slot(own, 0), crate::memo(root, 1), false);
    crate::publish(root, 0);
    crate::publish(root, 1);
    crate::write(address, 4096, 0x2468ace0);
    assert_eq!(crate::read(address, 0), 0x13579bdf);
    let observer = accord(
        root,
        AccordTarget::Task(TaskId::new(CONTROLLER.load(Ordering::Acquire))),
        Limits {
            operations: Operations::Nest(NestRights::INSPECT),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: mapping(root, PendRange::Inherit),
        },
    );
    let old = crate::memo(root, 0);
    ADDRESS.store(address.get(), Ordering::Release);
    OBSERVER.store(observer.get(), Ordering::Release);
    crate::lifecycle::return_pend(alias);
    assert_eq!(crate::info(old).state, CellState::Published);
    assert_eq!(crate::read(address, 0), 0x13579bdf);
    COPIED.store(1, Ordering::Release);
    until(&CLEANUP);
    assert_eq!(
        crate::info(crate::raw_memo(root, 0)).state,
        CellState::Vacant
    );
    assert!(cell::publish(crate::ptr(&old.to_bytes())).is_err());
    assert_eq!(
        crate::info(crate::raw_memo(root, 1)).state,
        CellState::Published
    );
    assert_eq!(crate::read(address, 4096), 0x2468ace0);
    crate::delete(CellObject::Pend(root));
    crate::lifecycle::return_pend(root);
    crate::delete(CellObject::Slot(own));
    unit::exit(78, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

extern "C" fn stale_read() -> ! {
    let value = crate::read(VirtAddr::new(ADDRESS.load(Ordering::Acquire)), 0);
    unit::exit(value as u64, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

pub(super) fn run() {
    let controller = crate::lifecycle::self_id();
    CONTROLLER.store(controller.get(), Ordering::Release);
    let a = crate::lifecycle::spawn(owner as *const () as usize);
    let b = crate::lifecycle::spawn(publisher as *const () as usize);
    PUBLISHER.store(b.get(), Ordering::Release);
    for task in [a, b] {
        crate::license::delegate(task);
        accord(
            get(),
            AccordTarget::Task(task),
            Limits {
                operations: Operations::Nest(NestRights::INSPECT | NestRights::RESERVE),
                modifiable: Modifiable::Yes,
                distributable: Distributable::Yes,
                scope: PendScope::Nest(NestScope::Whole),
            },
        );
    }
    unit::resume(UnitTarget::Task(a)).unwrap();
    unit::resume(UnitTarget::Task(b)).unwrap();
    until(&DELETED);
    let observer = PendId::mint(until(&OBSERVER));
    let address = VirtAddr::new(ADDRESS.load(Ordering::Acquire));
    let identity = id(observer);
    let first = crate::info(crate::raw_memo(observer, 0));
    let sibling = crate::info(crate::raw_memo(observer, 1));
    assert_eq!(first.state, CellState::Published);
    assert_eq!(sibling.state, CellState::Published);
    assert_eq!(crate::read(address, 0), 0x13579bdf);
    assert_eq!(crate::read(address, 4096), 0x2468ace0);
    EXIT.store(1, Ordering::Release);
    let deadline = room::clock() + 5_000_000_000;
    loop {
        let source = crate::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(a)));
        let position = crate::info(crate::raw_memo(observer, 0));
        assert_eq!(crate::info(crate::raw_memo(observer, 1)), sibling);
        if source.reclaimed {
            assert_eq!(
                position.state,
                CellState::Vacant,
                "source reclaimed before foreign withdrawal"
            );
        }
        if source.state == UnitState::Exited && source.reclaimed {
            assert!(position.generation > first.generation);
            assert_eq!(position.page, 0);
            break;
        }
        assert!(
            room::clock() < deadline,
            "foreign source cleanup stalled: {source:?}"
        );
        room::yield_();
    }
    assert_eq!(id(observer), identity);
    assert_eq!(crate::pend::inspect(observer).status, PendStatus::Active);
    assert_eq!(crate::read(address, 4096), 0x2468ace0);
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
    let probe = crate::lifecycle::spawn(stale_read as *const () as usize);
    unit::resume(UnitTarget::Task(probe)).unwrap();
    // Vacant retains its reservation: the fault waits until mapping Release.
    let deadline = room::clock() + 5_000_000_000;
    loop {
        let result = crate::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(probe)));
        assert_ne!(
            result.state,
            UnitState::Exited,
            "stale PTE survived: {result:?}"
        );
        if result.state == UnitState::Waiting {
            break;
        }
        assert!(
            room::clock() < deadline,
            "stale read did not fault: {result:?}"
        );
        room::yield_();
    }
    assert_eq!(crate::info(crate::raw_memo(observer, 1)), sibling);
    assert_eq!(crate::read(address, 4096), 0x2468ace0);
    assert!(crate::lifecycle::wait_task(a));
    assert_eq!(crate::lifecycle::result(a), (1, 77));
    crate::lifecycle::delete(a);
    crate::lifecycle::return_pend(observer);
    CLEANUP.store(1, Ordering::Release);
    assert!(crate::lifecycle::wait_task(probe));
    let result = crate::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(probe)));
    assert!(
        matches!(result.exit, Some((ExitCause::Fault, _))),
        "released mapping did not terminate the fault wait: {result:?}"
    );
    crate::lifecycle::delete(probe);
    assert!(crate::lifecycle::wait_task(b));
    assert_eq!(crate::lifecycle::result(b), (1, 78));
    crate::lifecycle::delete(b);
    writeln!(crate::Console, "PASS nest-source-exit foreign-cpu=1 deleted-slot=1 alias-return=1 alias-narrow=1 vacant-before-reclaim=1 stale-pte-fault=1 sibling=1 reservation=1").unwrap();
}
