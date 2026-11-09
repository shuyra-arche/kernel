use super::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::model::{
    identity::{Mark, TaskId},
    modifiable::Modifiable,
    nest::NestKind,
    pend::{
        AccordRequest, AccordTarget, Distributable, Limits, NestRights, Operations, PendRange,
        PortRights, ShareRights,
    },
    unit::UnitTarget,
};
use kernel_client::call::{pend, share_view, unit};
static BAR: AtomicUsize = AtomicUsize::new(0);
static PORT: AtomicUsize = AtomicUsize::new(0);
static NEST: AtomicUsize = AtomicUsize::new(0);
static MEMO: AtomicUsize = AtomicUsize::new(0);
fn delegate(root: PendId, task: TaskId, operations: Operations, modifiable: Modifiable) -> PendId {
    let bytes = AccordRequest {
        src: root,
        target: AccordTarget::Task(task),
        mark: Mark::NONE,
        limits: Limits {
            operations,
            modifiable,
            distributable: Distributable::No,
            range: PendRange::Inherit,
        },
    }
    .to_bytes();
    pend::accord(super::super::ptr(&bytes)).unwrap()
}
extern "C" fn publisher() -> ! {
    let bar_token = PendId::mint(BAR.load(Ordering::Acquire));
    let root = PendId::mint(NEST.load(Ordering::Acquire));
    let port = PendId::mint(PORT.load(Ordering::Acquire));
    let (bar, _) = share_view::open(bar_token).unwrap();
    nest::attach(root, port).unwrap();
    let slot = super::super::allocate(1);
    let pos = create(root, slot, ADDRESS, true);
    if let CellObject::Memo { mapping, .. } = pos.object {
        MEMO.store(mapping.get(), Ordering::Release);
    }
    publish(pos);
    // This old write is scheduled for a later QEMU timer callback. Returning the
    // source to a manager must wait until it cannot hit a replacement Page.
    start(bar, ADDRESS, true);
    assert_ne!(read(bar, 0x98) & 1, 0);
    unit::exit(77, 0, VirtAddr::new(0), 0)
}
pub(in super::super) fn death(at: usize, count: usize) {
    let (bar, port) = roots(at, count);
    let root = nest::create(crate::license::get(), NestKind::External).unwrap();
    let task = super::super::lifecycle::spawn(publisher as *const () as usize);
    crate::license::delegate(task);
    let bar_child = delegate(
        bar,
        task,
        Operations::Share(ShareRights::OPEN | ShareRights::SHUT | ShareRights::ASSOCIATE),
        Modifiable::Yes,
    );
    let port_child = delegate(
        port,
        task,
        Operations::Port(PortRights::ATTACH | PortRights::INSPECT),
        Modifiable::No,
    );
    let nest_child = delegate(
        root,
        task,
        Operations::Nest(NestRights::all()),
        Modifiable::Yes,
    );
    BAR.store(bar_child.get(), Ordering::Release);
    PORT.store(port_child.get(), Ordering::Release);
    NEST.store(nest_child.get(), Ordering::Release);
    let closed_event = sign::create(crate::license::get()).unwrap();
    sign::attach(closed_event, SignSource::NestClosed { token: root }, 0).unwrap();
    assert!(share_view::open(bar).is_err());
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(super::super::lifecycle::wait_task(task));
    assert_eq!(super::super::lifecycle::result(task), (1, 77));
    event(closed_event, SignSource::NestClosed { token: root });
    closed(root);
    let pos = CellPosition {
        object: CellObject::Memo {
            target: MappingTarget::Io(root),
            mapping: MappingId::from_bytes(&(MEMO.load(Ordering::Acquire) as u64).to_le_bytes())
                .unwrap(),
        },
        page: 0,
        generation: 1,
    };
    assert_eq!(super::super::info(pos).state, CellState::Ready);
    assert!(!super::super::info(pos).external);
    assert!(cell::publish(super::super::ptr(&current(pos).to_bytes())).is_err());
    let mut info = [0; NestInfo::WIDTH];
    assert!(nest::inspect(nest_child, VirtAddr::new(info.as_mut_ptr() as usize)).is_err());
    // Raw BAR authority must have returned independently before reassignment.
    let (registers, _) = share_view::open(bar).unwrap();
    assert_eq!(read(registers, 0x98) & 1, 0);
    let next = nest::create(crate::license::get(), NestKind::External).unwrap();
    nest::attach(next, port).unwrap();
    let slot = super::super::allocate(1);
    let (cpu, address) = super::super::attach(slot, 1);
    super::super::write(address, 0, 0xc3c3c3c3);
    super::super::retract(cpu, 0);
    let fresh = create(next, slot, ADDRESS, true);
    publish(fresh);
    transfer(registers, ADDRESS, false);
    assert_eq!(context(next).fault_sequence, 0);
    retract(fresh);
    super::super::handoff(super::super::slot(slot, 0), current(fresh), false);
    super::super::publish(cpu, 0);
    assert_eq!(super::super::read(address, 0), 0xc3c3c3c3);
    super::super::retract(cpu, 0);
    nest::detach(next).unwrap();
    closed(next);
    for pos in [pos, fresh] {
        super::super::delete(pos.object);
    }
    for token in [root, next] {
        nest::delete(token).unwrap();
        pend::return_(token).unwrap();
    }
    sign::delete(closed_event).unwrap();
    pend::return_(closed_event).unwrap();
    super::super::delete(super::super::raw_memo(cpu, 0).object);
    super::super::delete(CellObject::Slot(slot));
    share_view::shut(bar).unwrap();
    unit::delete(UnitTarget::Task(task)).unwrap();
    let _ = writeln!(
        super::super::Console,
        "PASS cell-io-exit owner=dead pending=1 retained=1 late-access=denied rebind=1"
    );
}
