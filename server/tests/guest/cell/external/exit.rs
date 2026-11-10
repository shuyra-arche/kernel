use super::*;
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
use kernel_api::{
    abi::call::{nest::NestFail, sign::SignReply},
    model::{identity::TaskId, pend::ShareRights, unit::UnitTarget},
};
use kernel_client::call::{share_view, unit};
static BAR: AtomicUsize = AtomicUsize::new(0);
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
    }.to_bytes();
    pend::accord(super::super::ptr(&bytes)).unwrap()
}
fn prepare(bar: PendId, root: PendId, task: TaskId) -> PendId {
    crate::license::delegate(task);
    let bar_child = delegate(
        bar,
        task,
        Operations::Share(ShareRights::OPEN | ShareRights::SHUT | ShareRights::ASSOCIATE),
        Modifiable::Yes,
    );
    let nest_child = delegate(root, task, limits().operations, Modifiable::Yes);
    BAR.store(bar_child.get(), Ordering::Release);
    NEST.store(nest_child.get(), Ordering::Release);
    nest_child
}
extern "C" fn publisher() -> ! {
    let bar_token = PendId::mint(BAR.load(Ordering::Acquire));
    let root = PendId::mint(NEST.load(Ordering::Acquire));
    let (bar, _) = share_view::open(bar_token).unwrap();
    let slot = super::super::allocate(1);
    let pos = create(root, slot, ADDRESS, true);
    if let CellObject::Memo { mapping, .. } = pos.object {
        MEMO.store(mapping.get(), Ordering::Release);
    }
    publish(pos);
    // Death occurs with a queued QEMU write, not with an already-idle device.
    start(bar, ADDRESS, true);
    assert_ne!(read(bar, 0x98) & 1, 0);
    unit::exit(77, 0, VirtAddr::new(0), 0)
}
extern "C" fn successor() -> ! {
    let bar_token = PendId::mint(BAR.load(Ordering::Acquire));
    let token = PendId::mint(NEST.load(Ordering::Acquire));
    let (bar, _) = share_view::open(bar_token).unwrap();
    assert_eq!(context(token).phase, NestPhase::Bound);
    let slot = super::super::allocate(1);
    let (cpu, address) = super::super::attach(slot, 1);
    super::super::write(address, 0, 0xc3c3c3c3);
    super::super::retract(cpu, 0);
    let fresh = create(token, slot, ADDRESS, true);
    publish(fresh);
    transfer(bar, ADDRESS, false);
    retract(fresh);
    super::super::delete(fresh.object);
    super::super::publish(cpu, 0);
    assert_eq!(super::super::read(address, 0), 0xc3c3c3c3);
    super::super::retract(cpu, 0);
    super::super::delete(super::super::raw_memo(cpu, 0).object);
    super::super::delete(CellObject::Slot(slot));
    share_view::shut(bar_token).unwrap();
    unit::exit(78, 0, VirtAddr::new(0), 0)
}
pub(in super::super) fn death(at: usize, count: usize) {
    let (bar, root) = roots(at, count);
    let identity = context(root).id;
    // An independent live leaf proves that temporary-holder death is not a
    // disguised full-context reset or replacement.
    let keeper_slot = super::super::allocate(1);
    let (keeper_cpu, keeper_address) = super::super::attach(keeper_slot, 1);
    super::super::write(keeper_address, 0, 0x12345678);
    super::super::retract(keeper_cpu, 0);
    let keeper = create(root, keeper_slot, ADDRESS + 4096, true);
    publish(keeper);
    let closed_event = sign::create(crate::license::get()).unwrap();
    sign::attach(closed_event, SignSource::NestClosed { token: root }, 0).unwrap();
    let task = super::super::lifecycle::spawn(publisher as *const () as usize);
    let nest_child = prepare(bar, root, task);
    assert!(share_view::open(bar).is_err());
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(super::super::lifecycle::wait_task(task));
    assert_eq!(super::super::lifecycle::result(task), (1, 77));
    let pos = CellPosition {
        object: CellObject::Memo {
            target: MappingTarget::Io(root),
            mapping: MappingId::from_bytes(&(MEMO.load(Ordering::Acquire) as u64).to_le_bytes()).unwrap(),
        },
        page: 0,
        generation: 1,
    };
    let until = room::clock() + 2_000_000_000;
    while observed(pos).is_some() {
        assert_eq!(context(root).id, identity);
        assert_eq!(context(root).phase, NestPhase::Bound);
        assert_eq!(super::super::info(keeper).state, CellState::Published);
        assert!(room::clock() < until, "dead branch retirement stalled");
        room::yield_();
    }
    super::super::lifecycle::delete(task);
    assert_eq!(context(root).id, identity);
    assert_eq!(context(root).phase, NestPhase::Bound);
    assert_eq!(sign::listen(closed_event, Wait::Poll).unwrap(), SignReply::Pending);
    assert_eq!(super::super::info(keeper).state, CellState::Published);
    let mut info = [0; NestInfo::WIDTH];
    assert!(nest::inspect(nest_child, VirtAddr::new(info.as_mut_ptr() as usize)).is_err());
    let (registers, _) = share_view::open(bar).unwrap();
    assert_eq!(read(registers, 0x98) & 1, 0);
    let faults = context(root).fault_sequence;
    assert!(faults > 0);
    assert_eq!(context(root).fault_address, ADDRESS as u64);
    // Losing the old mapping does not authorize the new Task to use its ID.
    assert!(observed(pos).is_none());
    share_view::shut(bar).unwrap();
    let next = super::super::lifecycle::spawn(successor as *const () as usize);
    prepare(bar, root, next);
    unit::resume(UnitTarget::Task(next)).unwrap();
    assert!(super::super::lifecycle::wait_task(next));
    assert_eq!(super::super::lifecycle::result(next), (1, 78));
    super::super::lifecycle::delete(next);
    assert_eq!(context(root).id, identity);
    assert_eq!(context(root).phase, NestPhase::Bound);
    assert_eq!(context(root).fault_sequence, faults);
    assert_eq!(super::super::info(keeper).state, CellState::Published);
    let (registers, _) = share_view::open(bar).unwrap();
    transfer(registers, ADDRESS + 4096, true);
    retract(keeper);
    super::super::delete(keeper.object);
    super::super::publish(keeper_cpu, 0);
    assert_eq!(super::super::read(keeper_address, 0), 0xc3c3c3c3);
    super::super::retract(keeper_cpu, 0);
    assert_eq!(sign::listen(closed_event, Wait::Poll).unwrap(), SignReply::Pending);
    assert_eq!(nest::delete(root).unwrap_err().source, NestFail::Busy);
    event(closed_event, SignSource::NestClosed { token: root });
    closed(root);
    nest::delete(root).unwrap();
    returned(root);
    sign::delete(closed_event).unwrap();
    returned(closed_event);
    super::super::delete(super::super::raw_memo(keeper_cpu, 0).object);
    super::super::delete(CellObject::Slot(keeper_slot));
    share_view::shut(bar).unwrap();
    let _ = writeln!(
        super::super::Console,
        "PASS cell-io-exit owner=dead pending=1 retained=1 late-access=denied handover=1 same-nest=1 sibling=1"
    );
}
