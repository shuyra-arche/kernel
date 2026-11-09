use crate::{allocate, attach, delete, ptr, raw_memo, retract};
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        nest::{NestInfo, NestKind},
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, NestRights, Operations, PendRange,
        },
        unit::{ControlId, ProgramKind, UnitCreate, UnitTarget},
    },
};
use kernel_client::call::{cell, nest, pend, share_view, unit};
core::arch::global_asm!(
 ".pushsection .text.nest_test,\"ax\"", ".balign 4", ".global nest_child_start", "nest_child_start:",
 "li a0, 83", "li a1, 0", "li a2, 0", "li a3, 0", "li a7, {exit}", "ebreak", "j .",
 ".global nest_child_end", "nest_child_end:", ".popsection",
 exit=const ((kernel_api::abi::VERSION as usize)<<48)|(1<<32)|5
);
unsafe extern "C" {
    static nest_child_start: u8;
    static nest_child_end: u8;
}
fn request(token: PendId) -> [u8; UnitCreate::WIDTH] {
    UnitCreate::Team {
        nest: Some(token),
        kind: ProgramKind::User,
        guard: ControlId::NONE,
    }
    .to_bytes()
}
pub(super) fn run() {
    build(false);
}
// Leave a whole Team of held Tasks to the Kernel, with no Join/Oust/receipt
// cleanup by their owner. This crosses the four-husk service limit and checks
// that cleared receipts cannot hide members whose retirement is still pending.
pub(super) extern "C" fn cascade() -> ! {
    build(true);
    unit::exit(84, 0, VirtAddr::new(0), 0)
}
fn build(cascade: bool) {
    let external = nest::create(crate::license::get(), NestKind::External).unwrap();
    assert!(unit::create(ptr(&request(external))).is_err());
    nest::delete(external).unwrap();
    pend::return_(external).unwrap();
    let root = nest::create(crate::license::get(), NestKind::Internal).unwrap();
    let mut observation = [0; NestInfo::WIDTH];
    nest::inspect(root, VirtAddr::new(observation.as_mut_ptr() as usize)).unwrap();
    assert_eq!(
        NestInfo::from_bytes(&observation).unwrap().kind,
        NestKind::Internal
    );
    let descriptor = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        limits: Limits {
            operations: Operations::Nest(NestRights::ATTACH),
            modifiable: Modifiable::Yes,
            distributable: Distributable::No,
            range: PendRange::Inherit,
        },
        mark: Mark::NONE,
    }
    .to_bytes();
    let child = pend::accord(ptr(&descriptor)).unwrap();
    assert!(unit::create(ptr(&request(child))).is_err());
    assert!(unit::create(ptr(&request(root))).is_err());
    pend::return_(child).unwrap();
    let UnitTarget::Team(team) = unit::create(ptr(&request(root))).unwrap() else {
        panic!("expected Team")
    };
    assert!(nest::inspect(root, VirtAddr::new(observation.as_mut_ptr() as usize)).is_err());
    assert!(unit::create(ptr(&request(root))).is_err());
    let reference = allocate(1);
    let (staging, address) = attach(reference, 1);
    let start = core::ptr::addr_of!(nest_child_start);
    let length = core::ptr::addr_of!(nest_child_end) as usize - start as usize;
    unsafe {
        core::ptr::copy_nonoverlapping(start, address.get() as *mut u8, length);
    }
    retract(staging, 0);
    delete(raw_memo(staging, 0).object);
    let source = share_view::create(crate::license::get(), reference, Modifiable::No).unwrap();
    let descriptor = CellCreate::Memo {
        source: CellSource::Pend {
            token: source,
            offset: 0,
            count: 1,
        },
        destination: MemoDestination::Internal {
            team,
            placement: Placement::lowest(4096).unwrap(),
            access: 10,
            control: MappingControl::Caller,
        },
    }
    .to_bytes();
    let CreatedCell::Memo { mapping, address } =
        cell::create(PendId::NONE, ptr(&descriptor)).unwrap()
    else {
        panic!("expected Memo")
    };
    let position = CellPosition {
        object: CellObject::Memo {
            target: MappingTarget::Cpu(team),
            mapping,
        },
        page: 0,
        generation: 2,
    }
    .to_bytes();
    // Association advances the initial vacant generation once.
    cell::publish(ptr(&position)).unwrap();
    let descriptor = UnitCreate::Task {
        team: Some(team),
        entry: address,
        args: VirtAddr::new(0),
        count: 0,
        stack: VirtAddr::new(0),
    }
    .to_bytes();
    let mut tasks = [TaskId::new(0); 8];
    for task in &mut tasks {
        let UnitTarget::Task(id) = unit::create(ptr(&descriptor)).unwrap() else {
            panic!("expected Task")
        };
        *task = id;
    }
    if cascade {
        return;
    }
    for task in tasks {
        unit::resume(UnitTarget::Task(task)).unwrap();
    }
    for task in tasks {
        assert!(crate::lifecycle::wait_task(task));
        assert_eq!(crate::lifecycle::result(task), (1, 83));
    }
    for (at, task) in tasks.iter().copied().enumerate() {
        crate::lifecycle::delete(task);
        // Reclaiming/removing one member must preserve every other receipt and
        // its immutable result, even after all executions have already exited.
        for other in &tasks[at + 1..] {
            assert_eq!(crate::lifecycle::result(*other), (1, 83));
        }
    }
    unit::finish(team).unwrap();
    let deadline = kernel_client::call::room::clock().saturating_add(5_000_000_000);
    loop {
        match unit::delete(UnitTarget::Team(team)) {
            Ok(()) => break,
            Err(error) => {
                assert_eq!(error.source, kernel_api::abi::call::unit::UnitFail::Busy);
                assert!(
                    kernel_client::call::room::clock() < deadline,
                    "Team retirement stalled"
                );
                kernel_client::call::room::yield_();
            }
        }
    }
    pend::return_(source).unwrap();
    delete(CellObject::Slot(reference));
}
