//! A different Team controls exactly one foreign Memo position, without joining
//! that Team or receiving its Nest, owning Flat, or publication permission.
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        identity::PendId,
        modifiable::Modifiable,
        pend::{AccordTarget, NestRights, Operations},
        unit::{ControlId, CreatedUnit, ProgramKind, UnitCreate, UnitTarget},
    },
};
use kernel_client::call::{nest, share_view, unit};
const _: () = assert!(kernel_api::model::pend::PendInfo::SIZE <= 128);
const _: () = assert!(CellPosition::WIDTH == 32 && CellInfo::WIDTH <= 64);
const fn call(class: usize, slot: usize) -> usize {
    ((kernel_api::abi::VERSION as usize) << 48) | (class << 32) | slot
}
core::arch::global_asm!(include_str!("worker.S"),collect=const call(7,18),inspect=const call(2,7),remove=const call(2,11),retract=const call(2,5),publish=const call(2,4),yield_=const call(0,0),exit=const call(1,5));
unsafe extern "C" {
    static memo_remote_start: u8;
    static memo_remote_end: u8;
}
pub(super) fn run(target: PendId) {
    let nest = nest::create(crate::license::get()).unwrap();
    let descriptor = UnitCreate::Team {
        member_exit: false,
        nest: Some(nest),
        kind: ProgramKind::User,
        guard: ControlId::NONE,
    }
    .to_bytes();
    let CreatedUnit::Team { team, nest: whole } = unit::create(crate::ptr(&descriptor)).unwrap()
    else {
        panic!("Team expected");
    };
    let code = crate::allocate(1);
    let (staging, address) = crate::attach(code, 1);
    let start = core::ptr::addr_of!(memo_remote_start);
    let length = core::ptr::addr_of!(memo_remote_end).addr() - start.addr();
    assert!(length < 4096);
    unsafe {
        core::ptr::copy_nonoverlapping(start, address.get() as *mut u8, length);
    }
    crate::retract(staging, 0);
    crate::delete(crate::raw_memo(staging, 0).object);
    let share = share_view::create(crate::license::get(), code, Modifiable::No).unwrap();
    let (execution, address) = crate::space::reserve(
        whole,
        Placement::lowest(4096).unwrap(),
        1,
        CellLevel::BASE,
        10,
    );
    crate::handoff(crate::raw_memo(share, 0), crate::memo(execution, 0), false);
    crate::publish(execution, 0);
    let descriptor = UnitCreate::Task {
        team: Some(team),
        entry: address,
        args: VirtAddr::new(0),
        count: 0,
        stack: VirtAddr::new(0),
    }
    .to_bytes();
    let CreatedUnit::Task(task) = unit::create(crate::ptr(&descriptor)).unwrap() else {
        panic!("Task expected");
    };
    super::grant(
        target,
        AccordTarget::Task(task),
        Operations::Nest(NestRights::INSPECT | NestRights::RETRACT),
        1,
        1,
    );
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(crate::lifecycle::wait_task(task));
    assert_eq!(crate::lifecycle::result(task), (1, 136));
    crate::lifecycle::delete(task);
    unit::finish(team).unwrap();
    let end = kernel_client::call::room::clock() + 2_000_000_000;
    loop {
        match unit::delete(UnitTarget::Team(team)) {
            Ok(()) => break,
            Err(e) => {
                assert_eq!(e.source, kernel_api::abi::call::unit::UnitFail::Busy);
                assert!(kernel_client::call::room::clock() < end);
                kernel_client::call::room::yield_();
            }
        }
    }

    crate::lifecycle::return_pend(execution);
    crate::lifecycle::return_pend(whole);
    crate::lifecycle::return_pend(share);
    crate::delete(CellObject::Slot(code));
    assert_eq!(
        crate::info(crate::raw_memo(target, 0)).state,
        CellState::Published
    );
    assert_eq!(
        crate::info(crate::raw_memo(target, 1)).state,
        CellState::Ready
    );
    crate::publish(target, 1);
}
