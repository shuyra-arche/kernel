//! Memo authorization and window blocking use only the public ABI.
mod remote;
use crate::{allocate, attach, delete, handoff, info, memo, ptr, raw_memo, slot};
use kernel_api::{
    address::VirtAddr,
    model::{
        cell::*,
        identity::{Mark, PendId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Distributable, FlatRights, Limits, NarrowRequest,
            NestRights, Operations, PendInfo, PendRange,
        },
        unit::{ExitScope, UnitTarget},
    },
};
use kernel_client::call::{cell, flat, pend, room, unit};
fn grant(
    root: PendId,
    target: AccordTarget,
    operations: Operations,
    offset: usize,
    count: usize,
) -> PendId {
    pend::accord(ptr(&AccordRequest {
        src: root,
        target,
        mark: Mark::NONE,
        limits: Limits {
            operations,
            modifiable: Modifiable::Yes,
            distributable: Distributable::No,
            scope: crate::space::range(
                crate::pend::inspect(root).limits.scope,
                PendRange::Pages { offset, count },
            ),
        },
    }
    .to_bytes()))
    .unwrap()
}
fn block(token: PendId) {
    let end = room::clock() + 2_000_000_000;
    loop {
        match flat::block(token) {
            Ok(()) => break,
            Err(e) => {
                assert_eq!(e.source, kernel_api::abi::call::flat::FlatFail::Busy);
                assert!(room::clock() < end, "Block did not settle");
                room::yield_();
            }
        }
    }
}
extern "C" fn recipient() -> ! {
    let mut words = [0; PendInfo::WORDS];
    assert_eq!(
        pend::collect(PendId::NONE, VirtAddr::new(words.as_mut_ptr() as usize), 1).unwrap(),
        1
    );
    let token = PendInfo::from_words(words).unwrap().token;
    let position = memo(token, 0);
    assert!(cell::remove(ptr(&position.to_bytes())).is_err());
    assert!(cell::publish(ptr(&position.to_bytes())).is_err());
    crate::lifecycle::return_pend(token);
    unit::exit(136, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
pub(super) fn run() {
    let root = flat::create(crate::license::get(), 8192, 4096, 0).unwrap();
    let owner = flat::slice(root, 0, 2).unwrap();
    let (mapping, address) = attach(owner, 2);
    crate::write(address, 4096, 0x136);
    remote::run(mapping);
    let reader = grant(
        mapping,
        AccordTarget::Caller,
        Operations::Nest(NestRights::INSPECT),
        1,
        1,
    );
    assert_eq!(
        info(raw_memo(reader, 0)).page,
        info(raw_memo(mapping, 1)).page
    );
    let mut out = [0; CellInfo::WIDTH];
    assert!(
        cell::inspect(
            ptr(&raw_memo(reader, 1).to_bytes()),
            VirtAddr::new(out.as_mut_ptr() as usize)
        )
        .is_err()
    );
    assert!(cell::remove(ptr(&memo(reader, 0).to_bytes())).is_err());
    assert!(kernel_client::call::nest::release(reader).is_err());
    let task = crate::lifecycle::spawn(recipient as *const () as usize);
    grant(
        mapping,
        AccordTarget::Task(task),
        Operations::Nest(NestRights::INSPECT),
        1,
        1,
    );
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert!(crate::lifecycle::wait_task(task));
    assert_eq!(crate::lifecycle::result(task), (1, 136));
    crate::lifecycle::delete(task);
    let manager = grant(
        mapping,
        AccordTarget::Caller,
        Operations::Nest(NestRights::mapping()),
        1,
        1,
    );
    assert!(kernel_client::call::nest::release(manager).is_err());
    crate::retract(mapping, 1);
    let before = memo(manager, 0);
    cell::remove(ptr(&before.to_bytes())).unwrap();
    assert_eq!(info(raw_memo(mapping, 1)).state, CellState::Vacant);
    assert!(cell::remove(ptr(&before.to_bytes())).is_err());
    assert_eq!(info(slot(owner, 1)).state, CellState::Ready);
    handoff(slot(owner, 1), memo(manager, 0), false);
    assert!(cell::publish(ptr(&before.to_bytes())).is_err());
    crate::publish(manager, 0);
    crate::lifecycle::return_pend(manager);
    assert_eq!(info(raw_memo(mapping, 1)).state, CellState::Ready);
    crate::publish(mapping, 1);
    let blocked = grant(
        root,
        AccordTarget::Caller,
        Operations::Flat(
            FlatRights::BLOCK | FlatRights::ASSOCIATE | FlatRights::TRANSFER | FlatRights::INSPECT,
        ),
        1,
        1,
    );
    let selected = flat::slice(blocked, 0, 1).unwrap();
    let (alias, _) = crate::reserve(owner, 2);
    block(blocked);
    let bypass = CellAssociation {
        source: memo(mapping, 1),
        target: memo(alias, 1),
    }
    .to_bytes();
    assert!(cell::insert(ptr(&bypass)).is_err());
    assert_eq!(info(raw_memo(alias, 1)).state, CellState::Vacant);
    delete(CellObject::Pend(alias));
    assert_eq!(info(raw_memo(mapping, 0)).state, CellState::Published);
    assert_eq!(info(raw_memo(mapping, 1)).state, CellState::Ready);
    assert!(cell::publish(ptr(&memo(mapping, 1).to_bytes())).is_err());
    flat::allow(root).unwrap(); // Only the exact blocking branch can reopen.
    assert!(cell::publish(ptr(&memo(mapping, 1).to_bytes())).is_err());
    let empty = allocate(1);
    let denied = CellHandoff {
        local: slot(owner, 1),
        remote: slot(empty, 0),
    }
    .to_bytes();
    assert!(cell::push(ptr(&denied)).is_err());
    handoff(slot(selected, 0), slot(empty, 0), false);
    assert_eq!(info(slot(owner, 1)).state, CellState::Vacant);
    handoff(slot(empty, 0), slot(selected, 0), true);
    // Removal changes only the association. The exact block holder can rebuild it.
    cell::remove(ptr(&memo(mapping, 1).to_bytes())).unwrap();
    handoff(slot(selected, 0), memo(mapping, 1), false);
    flat::allow(blocked).unwrap();
    crate::publish(mapping, 1);
    assert_eq!(crate::read(address, 4096), 0x136);
    block(blocked);
    // Revocation releases this branch's gate after normal CPU withdrawal.
    crate::lifecycle::return_pend(blocked);
    crate::publish(mapping, 1);
    crate::lifecycle::return_pend(reader);
    let narrow = NarrowRequest {
        token: mapping,
        limits: Limits {
            operations: Operations::Nest(NestRights::INSPECT | NestRights::RETRACT),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: crate::space::mapping(
                mapping,
                PendRange::Pages {
                    offset: 0,
                    count: 1,
                },
            ),
        },
    }
    .to_bytes();
    pend::narrow(ptr(&narrow)).unwrap();
    assert!(
        cell::inspect(
            ptr(&raw_memo(reader, 0).to_bytes()),
            VirtAddr::new(out.as_mut_ptr() as usize)
        )
        .is_err()
    );
    crate::lifecycle::return_pend(mapping);
    delete(CellObject::Slot(selected));
    delete(CellObject::Slot(empty));
    delete(CellObject::Slot(owner));
    flat::delete(root).unwrap();
    crate::lifecycle::return_pend(root);
    use core::fmt::Write;
    let _ = writeln!(
        crate::Console,
        "PASS memo-authority window=1 delegate=1 foreign-team=1 revoke=1 remove=1 stale=1 block=1 transfer=1 allow=1"
    );
}
