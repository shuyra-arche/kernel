//! Production lineage checks against the real Task ledger/Pend. Namespace
//! lifetime is a fixture; these tests do not execute Memo or hardware cleanup.
use super::{
    allocation, basis, ledger,
    reservation::{id, map, setup},
    support::{Nest, Task},
};
use crate::basis::pend;
use kernel_api::{
    abi::call::{cell::CellFail, pend::PendFail},
    address::PageCount,
    model::{
        cell::CellLevel,
        identity::{PendId, TaskId},
        modifiable::Modifiable,
        pend::{Distributable, NestRights, Operations},
    },
};
use std::sync::{Arc, Weak, atomic::Ordering};
#[allow(dead_code)]
#[path = "../../src/basis/nest/memo/source/lineage.rs"]
mod lineage;
use lineage::Lineage;

fn failure(error: PendFail) -> CellFail {
    match error {
        PendFail::OutOfMemory => CellFail::OutOfMemory,
        PendFail::Busy => CellFail::Busy,
        _ => CellFail::Denied,
    }
}
fn grant(
    access: Modifiable,
    distribution: Distributable,
) -> (Arc<Task>, Arc<Nest>, Arc<ledger::PendEntry>) {
    let (task, nest, whole) = setup(access, distribution);
    let entry = {
        let _serial = task.pend.lock();
        let prepared =
            ledger::prepare_reservation(&task, &whole, id(1), PageCount::new(8).unwrap()).unwrap();
        let entry = prepared.entry().clone();
        nest.install(
            id(1),
            PageCount::new(8).unwrap(),
            CellLevel::BASE,
            entry.authority(),
        );
        prepared.publish();
        entry
    };
    (task, nest, entry)
}
fn extend(
    previous: Option<Arc<Lineage>>,
    task: &Arc<Task>,
    nest: &Nest,
    entry: &Arc<ledger::PendEntry>,
) -> Result<Arc<Lineage>, CellFail> {
    let _commit = basis::commit();
    Lineage::extend(
        previous,
        task,
        entry,
        Operations::Nest(NestRights::PUBLISH),
        map(1, 2, 2),
        nest.reservation(id(1)).as_ref(),
    )
}

#[test]
fn multi_hop_readonly_route_cannot_be_upgraded_and_middle_revocation_is_live() {
    let (one, first, reader) = grant(Modifiable::No, Distributable::Yes);
    let (two, second, writer) = grant(Modifiable::Yes, Distributable::Yes);
    let original = extend(None, &one, &first, &reader).unwrap();
    let exported = extend(Some(original.clone()), &two, &second, &writer).unwrap();
    assert_eq!(exported.check(Modifiable::No), Ok(()));
    assert_eq!(exported.check(Modifiable::Yes), Err(CellFail::Denied));
    assert!(exported.any(|hop| hop.branch(reader.token())));
    assert!(exported.any(|hop| hop.branch(writer.token())));
    writer.invalidate();
    assert_eq!(exported.check(Modifiable::No), Err(CellFail::Denied));
    assert_eq!(original.check(Modifiable::No), Ok(()));
    assert_eq!(writer.authority().settle(), Err(PendFail::Busy));
    drop(exported);
    writer.authority().settle().unwrap();
}

#[test]
fn foreign_ledger_and_nondistributable_middle_do_not_create_dependencies() {
    let (one, first, entry) = grant(Modifiable::Yes, Distributable::Yes);
    let (two, second, denied) = grant(Modifiable::Yes, Distributable::No);
    assert_eq!(
        extend(None, &two, &first, &entry).err(),
        Some(CellFail::Denied)
    );
    assert!(!entry.authority().external_pending());
    let original = extend(None, &one, &first, &entry).unwrap();
    assert_eq!(
        extend(Some(original.clone()), &two, &second, &denied).err(),
        Some(CellFail::Denied)
    );
    assert!(!denied.authority().external_pending());
    assert_eq!(original.check(Modifiable::Yes), Ok(()));
}

#[test]
fn closing_an_intermediate_reservation_or_task_invalidates_downstream() {
    for close_task in [false, true] {
        let (one, first, original) = grant(Modifiable::Yes, Distributable::Yes);
        let (two, second, middle) = grant(Modifiable::Yes, Distributable::Yes);
        let origin = extend(None, &one, &first, &original).unwrap();
        let exported = extend(Some(origin.clone()), &two, &second, &middle).unwrap();
        if close_task {
            two.dead.store(true, Ordering::Release);
        } else {
            second
                .reservation(id(1))
                .unwrap()
                .open
                .store(false, Ordering::Release);
        }
        assert_eq!(exported.check(Modifiable::No), Err(CellFail::Denied));
        assert!(exported.any(Lineage::retiring));
        assert_eq!(origin.check(Modifiable::Yes), Ok(()));
    }
}

#[test]
fn failed_allocation_and_depth_limit_leave_no_unowned_export_guard() {
    let (task, nest, entry) = grant(Modifiable::Yes, Distributable::Yes);
    assert_eq!(
        allocation::fail_after(0, || extend(None, &task, &nest, &entry)).err(),
        Some(CellFail::OutOfMemory)
    );
    assert!(!entry.authority().external_pending());
    let mut chain = extend(None, &task, &nest, &entry).unwrap();
    for _ in 1..64 {
        chain = extend(Some(chain), &task, &nest, &entry).unwrap();
    }
    assert_eq!(
        extend(Some(chain.clone()), &task, &nest, &entry).err(),
        Some(CellFail::Denied)
    );
    assert_eq!(chain.check(Modifiable::Yes), Ok(()));
    drop(chain);
    assert!(!entry.authority().external_pending());
}

#[test]
fn exported_lineage_does_not_keep_task_or_reservation_alive() {
    let (task, nest, entry) = grant(Modifiable::Yes, Distributable::Yes);
    let chain = extend(None, &task, &nest, &entry).unwrap();
    let weak_task = Arc::downgrade(&task);
    let weak_nest = Arc::downgrade(&nest);
    drop(entry);
    drop(nest);
    drop(task);
    assert!(weak_task.upgrade().is_none());
    assert!(weak_nest.upgrade().is_none());
    assert_eq!(chain.check(Modifiable::No), Err(CellFail::Denied));
}
