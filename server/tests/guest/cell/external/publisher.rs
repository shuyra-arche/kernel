//! Completion belongs to the live publication holder, not the reserving Task.
use super::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};
use kernel_api::{
    abi::call::nest::NestFail,
    model::{
        pend::PendRange,
        unit::{ExitScope, UnitTarget},
    },
};
use kernel_client::call::unit;

static TOKEN: AtomicUsize = AtomicUsize::new(0);
static PHASE: AtomicU32 = AtomicU32::new(0);

fn advance(phase: u32) {
    PHASE.store(phase, Ordering::Release);
    room::wake_value(VirtAddr::new(PHASE.as_ptr() as usize), true).unwrap();
}

fn await_phase(phase: u32, wait: Wait) {
    loop {
        let observed = PHASE.load(Ordering::Acquire);
        if observed >= phase {
            return;
        }
        if let Wait::At(deadline) = wait {
            assert!(
                room::clock() < deadline,
                "publisher phase {observed} < {phase}"
            );
        }
        crate::room::wait(&PHASE, observed, wait);
    }
}

extern "C" fn publish_delegated() -> ! {
    let token = PendId::mint(TOKEN.load(Ordering::Acquire));
    let position = crate::memo(token, 0);
    let published = sign::create(crate::license::get()).unwrap();
    sign::attach(published, source(position, true), position.generation + 1).unwrap();
    // Task setup is ordinary work, outside the hardware completion deadline.
    // The process watchdog still bounds a setup that cannot make progress.
    advance(1);
    await_phase(2, Wait::Infinite);
    cell::publish(crate::ptr(&position.to_bytes())).unwrap();
    event(published, source(position, true));
    assert_eq!(crate::info(position).state, CellState::Published);
    advance(3);
    await_phase(4, Wait::Infinite);
    let position = current(position);
    let retired = sign::create(crate::license::get()).unwrap();
    sign::attach(retired, source(position, false), position.generation + 1).unwrap();
    begin_retract(position);
    event(retired, source(position, false));
    let observed = crate::info(position);
    assert_eq!(observed.state, CellState::Ready);
    assert!(!observed.external);
    for endpoint in [published, retired] {
        sign::delete(endpoint).unwrap();
        returned(endpoint);
    }
    returned(token);
    advance(5);
    unit::exit(79, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

pub(super) fn run(bar: VirtAddr, root: PendId) {
    let identity = context(root).id;
    let slot = crate::allocate(1);
    let (staging, address) = crate::attach(slot, 1);
    crate::write(address, 0, 0x12345678);
    crate::retract(staging, 0);
    crate::delete(CellObject::Pend(staging));
    returned(staging);
    let position = create(root, slot, ADDRESS, false);
    let CellObject::Pend(mapping) = position.object else {
        unreachable!()
    };
    let mapping_id = crate::space::id(mapping);
    let task = crate::lifecycle::spawn(publish_delegated as *const () as usize);
    crate::license::delegate(task);
    let child = crate::space::accord(
        mapping,
        AccordTarget::Task(task),
        Limits {
            operations: Operations::Nest(
                NestRights::INSPECT | NestRights::PUBLISH | NestRights::RETRACT,
            ),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: crate::space::mapping(mapping, PendRange::Inherit),
        },
    );
    TOKEN.store(child.get(), Ordering::Release);
    unit::resume(UnitTarget::Task(task)).unwrap();
    await_phase(1, Wait::Infinite);
    let deadline = room::clock() + 2_000_000_000;
    advance(2);
    await_phase(3, Wait::At(deadline));
    assert_eq!(crate::info(position).state, CellState::Published);
    transfer(bar, ADDRESS, false);
    start(bar, ADDRESS, false);
    assert_ne!(
        read(bar, 0x98) & 1,
        0,
        "DMA must still be queued at Retract"
    );
    advance(4);
    assert!(crate::lifecycle::wait_task(task));
    assert_eq!(crate::lifecycle::result(task), (1, 79));
    crate::lifecycle::delete(task);
    assert_eq!(PHASE.load(Ordering::Acquire), 5);
    assert_eq!(read(bar, 0x98) & 1, 0);
    assert_eq!(context(root).id, identity);
    assert_eq!(context(root).phase, NestPhase::Bound);
    assert_eq!(crate::space::id(mapping), mapping_id);
    assert_eq!(crate::info(position).state, CellState::Ready);
    assert!(!crate::info(position).external);
    assert_eq!(
        nest::reserve(
            root,
            Placement::exact(VirtAddr::new(ADDRESS), 4096).unwrap(),
            PageCount::new(1).unwrap(),
            CellLevel::BASE,
            2,
        )
        .unwrap_err()
        .source,
        NestFail::AlreadyMapped
    );
    crate::delete(position.object);
    returned(mapping);
    let (cpu, address) = crate::attach(slot, 1);
    assert_eq!(crate::read(address, 0), 0x12345678);
    crate::retract(cpu, 0);
    crate::delete(CellObject::Pend(cpu));
    returned(cpu);
    crate::delete(CellObject::Slot(slot));
    writeln!(
        crate::Console,
        "PASS external-publisher reserver-diff=1 source-diff=1 holder-events=2 queued=1 fenced=1"
    )
    .unwrap();
}
