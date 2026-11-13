use crate::{allocate, attach, delete, ptr, raw_memo, retract};
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        nest::{NestInfo, NestKind},
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, NestRights, NestScope, Operations,
            PendScope,
        },
        unit::{
            ControlId, CreatedUnit, ExitCause, ExitScope, UnitCreate, UnitQuery,
            UnitState, UnitTarget,
        },
    },
};
use kernel_client::call::{nest, pend, share_view, unit};
core::arch::global_asm!(
 ".pushsection .text.nest_test,\"ax\"", ".balign 4", ".global nest_child_start", "nest_child_start:",
 "li a0, 83", "li a1, 0", "li a2, 0", "li a3, 0", "li a4, 0", "li a7, {exit}", "ebreak", "j .",
 ".global nest_blocked_start", "nest_blocked_start:",
 "li a0, -1", "li a7, {sleep}", "ebreak", "j .",
 ".global nest_team_exit_start", "nest_team_exit_start:",
 "li a0, 23", "li a1, 0", "li a2, 0", "li a3, 0", "li a4, 1", "li a7, {exit}", "ebreak", "j .",
 ".global nest_child_end", "nest_child_end:", ".popsection",
 exit=const ((kernel_api::abi::VERSION as usize)<<48)|(1<<32)|5,
 sleep=const ((kernel_api::abi::VERSION as usize)<<48)|1,
);
unsafe extern "C" {
    static nest_child_start: u8;
    static nest_blocked_start: u8;
    static nest_team_exit_start: u8;
    static nest_child_end: u8;
}
fn request(token: PendId, member_exit: bool) -> [u8; UnitCreate::WIDTH] {
    UnitCreate::Team {
        member_exit,
        nest: Some(token),
        guard: ControlId::NONE,
    }
    .to_bytes()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    TaskExit,
    TeamExit,
    Cascade,
}

pub(super) fn run() {
    use kernel_api::abi::{
        call::unit::{UnitFail, operations::Exit},
        operation::RegisterCall,
    };
    let denied = Exit {
        value: 23,
        reason: 0,
        note: VirtAddr::new(0),
        len: 0,
        scope: ExitScope::Team,
    };
    let (code, _, _) = unsafe { kernel_client::transport::trap(Exit::SLOT, denied.pack()) };
    assert_eq!(code, UnitFail::Denied.code() as usize);
    build(Case::TaskExit);
    build(Case::TeamExit);
}
// Leave a whole Team of held Tasks to the Kernel, with no Join/Oust/receipt
// cleanup by their owner. This crosses the four-husk service limit and checks
// that cleared receipts cannot hide members whose retirement is still pending.
pub(super) extern "C" fn cascade() -> ! {
    build(Case::Cascade);
    unit::exit(84, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn build(case: Case) {
    let member_exit = case == Case::TeamExit;
    // Ordinary creation has no External kind or requester arguments. External
    // Team-adoption rejection is exercised against the real trusted I/O grant.
    let root = nest::create(crate::license::get()).unwrap();
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
            scope: PendScope::Nest(NestScope::Whole),
        },
        mark: Mark::NONE,
    }
    .to_bytes();
    let child = pend::accord(ptr(&descriptor)).unwrap();
    assert!(unit::create(ptr(&request(child, member_exit))).is_err());
    assert!(unit::create(ptr(&request(root, member_exit))).is_err());
    pend::return_(child).unwrap();
    let CreatedUnit::Team {
        team,
        nest: admitted,
    } = unit::create(ptr(&request(root, member_exit))).unwrap()
    else {
        panic!("expected Team")
    };
    assert_eq!(admitted, root);
    nest::inspect(root, VirtAddr::new(observation.as_mut_ptr() as usize)).unwrap();
    assert_eq!(
        crate::pend::inspect(root).limits.operations,
        Operations::Nest(NestRights::INSPECT | NestRights::RESERVE)
    );
    assert!(unit::create(ptr(&request(root, member_exit))).is_err());
    let reference = allocate(1);
    let (staging, address) = attach(reference, 1);
    let start = core::ptr::addr_of!(nest_child_start);
    let length = core::ptr::addr_of!(nest_child_end) as usize - start as usize;
    unsafe { core::ptr::copy_nonoverlapping(start, address.get() as *mut u8, length) };
    retract(staging, 0);
    delete(raw_memo(staging, 0).object);
    let source = share_view::create(crate::license::get(), reference, Modifiable::No).unwrap();
    let (mapping, address) = crate::space::reserve(
        admitted,
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        10,
    );
    assert_eq!(crate::info(raw_memo(mapping, 0)).state, CellState::Vacant);
    crate::handoff(raw_memo(source, 0), crate::memo(mapping, 0), false);
    crate::publish(mapping, 0);
    let mut tasks = [TaskId::new(0); 8];
    let tasks = if member_exit {
        &mut tasks[..2]
    } else {
        &mut tasks[..]
    };
    let descriptor = |entry| {
        UnitCreate::Task {
            team: Some(team),
            entry,
            args: VirtAddr::new(0),
            count: 0,
            stack: VirtAddr::new(0),
        }
        .to_bytes()
    };
    for (index, task) in tasks.iter_mut().enumerate() {
        let entry = if member_exit {
            let symbol = if index == 0 {
                core::ptr::addr_of!(nest_blocked_start)
            } else {
                core::ptr::addr_of!(nest_team_exit_start)
            };
            address + (symbol.addr() - start.addr())
        } else {
            address
        };
        let CreatedUnit::Task(id) = unit::create(ptr(&descriptor(entry))).unwrap() else {
            panic!("expected Task")
        };
        *task = id;
    }
    if case == Case::Cascade {
        return;
    }
    let deadline = kernel_client::call::room::clock().saturating_add(5_000_000_000);
    if member_exit {
        let blocked = UnitTarget::Task(tasks[0]);
        unit::resume(blocked).unwrap();
        while crate::lifecycle::inspect(UnitQuery::Target(blocked)).state != UnitState::Waiting {
            assert!(
                kernel_client::call::room::clock() < deadline,
                "Team member did not block"
            );
            kernel_client::call::room::yield_();
        }
    }
    for task in tasks.iter().copied().skip(usize::from(member_exit)) {
        unit::resume(UnitTarget::Task(task)).unwrap();
    }
    for task in tasks.iter().copied() {
        assert!(crate::lifecycle::wait_task(task));
    }
    if member_exit {
        let target = UnitTarget::Team(team);
        let before = crate::lifecycle::inspect(UnitQuery::Target(target));
        assert_eq!(before.state, UnitState::Exited);
        assert!(!before.admitting);
        assert_eq!(before.exit, Some((ExitCause::Reap, 0)));
        assert_eq!(before.result, Some(23));
        assert_eq!(crate::lifecycle::result(tasks[0]), (0, 0));
        assert_eq!(crate::lifecycle::result(tasks[1]), (1, 23));
        assert!(unit::create(ptr(&descriptor(address))).is_err());
        unit::stop(target).unwrap();
        assert_eq!(crate::lifecycle::inspect(UnitQuery::Target(target)), before);
    }
    for (at, task) in tasks.iter().copied().enumerate() {
        if !member_exit {
            assert_eq!(crate::lifecycle::result(task), (1, 83));
        }
        crate::lifecycle::delete(task);
        // Removing one member preserves all other retained results.
        for other in &tasks[at + 1..] {
            let expected = if member_exit { (1, 23) } else { (1, 83) };
            assert_eq!(crate::lifecycle::result(*other), expected);
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
    crate::lifecycle::return_pend(mapping);
    crate::lifecycle::return_pend(admitted);
    pend::return_(source).unwrap();
    delete(CellObject::Slot(reference));
}
