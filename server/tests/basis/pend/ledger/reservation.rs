use super::{
    allocation, basis, ledger,
    support::{Nest, Task},
    target::Target,
};
use kernel_api::{
    abi::call::pend::PendFail,
    address::PageCount,
    model::{
        identity::{Mark, TaskId},
        modifiable::Modifiable,
        nest::ReservationId,
        pend::{
            Distributable, Limits, NestRights, NestScope, Operations, PendRange, PendScope,
            PendStatus,
        },
    },
};
use std::sync::{Arc, atomic::Ordering};

pub(super) fn id(raw: usize) -> ReservationId {
    ReservationId::from_bytes(&(raw as u64).to_le_bytes()).unwrap()
}
pub(super) fn map(raw: usize, offset: usize, count: usize) -> PendScope {
    PendScope::Nest(NestScope::Reservation {
        id: id(raw),
        range: PendRange::Pages { offset, count },
    })
}
pub(super) fn setup(
    modifiable: Modifiable,
    distributable: Distributable,
) -> (Arc<Task>, Arc<Nest>, Arc<ledger::PendEntry>) {
    let task = Task::new(1);
    let nest = Nest::new(1);
    let root = Arc::new(
        ledger::PendEntry::try_new(
            Target::Nest {
                owner: task.ident.id,
                nest: nest.clone(),
            },
            Mark::NONE,
            modifiable,
            None,
        )
        .unwrap(),
    );
    root.narrow(Limits {
        operations: Operations::Nest(NestRights::INSPECT | NestRights::RESERVE),
        distributable,
        ..root.authority().limits()
    })
    .unwrap();
    {
        let _serial = task.pend.lock();
        task.pend.prepare(root.clone()).unwrap().publish();
    }
    (task, nest, root)
}

#[test]
fn licensed_reservation_root_retains_parent_limits_and_actual_creation_edge() {
    let (task, nest, parent) = setup(Modifiable::No, Distributable::No);
    let serial = task.pend.lock();
    let prepared =
        ledger::prepare_reservation(&task, &parent, id(1), PageCount::new(8).unwrap()).unwrap();
    let entry = prepared.entry().clone();
    assert_eq!(entry.scope(), map(1, 0, 8));
    assert_eq!(
        entry.authority().limits(),
        Limits {
            operations: Operations::Nest(NestRights::reservation()),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: map(1, 0, 8),
        }
    );
    assert_eq!(entry.sire(), Some(parent.token()));
    assert!(entry.lord().ptr_eq(&Arc::downgrade(&task)));
    assert!(entry.target().same(parent.target()));
    assert_eq!(entry.target().owner(), parent.target().owner());
    assert!(entry.authority().alive());
    assert!(!entry.alive()); // The fresh Reservation has not been committed by Namespace yet.
    assert!(task.pend.entries.lock().get(entry.token()).is_none());
    assert!(task.pend.heirs.lock().is_empty());
    nest.reservations
        .lock()
        .unwrap()
        .push((id(1), PendStatus::Active));
    {
        let _commit = basis::commit();
        allocation::fail_after(0, || prepared.publish());
    }
    assert!(entry.alive());
    assert!(Arc::ptr_eq(
        task.pend.entries.lock().get(entry.token()).unwrap(),
        &entry
    ));
    let heirs = task.pend.heirs.lock();
    let [(source, receiver, token)] = &**heirs else {
        panic!("one actual creation edge")
    };
    assert_eq!(*source, parent.token());
    assert_eq!(*token, entry.token());
    assert!(receiver.ptr_eq(&Arc::downgrade(&task)));
    drop(heirs);
    drop(serial);
    assert_eq!(ledger::vestor(&entry), Some(task.ident.id));
    parent.invalidate();
    assert!(!entry.authority().alive());
    assert!(!entry.permits(Operations::Nest(NestRights::PUBLISH)));
}

#[test]
fn mapping_preparation_rejects_foreign_dead_and_reserve_less_whole_grants() {
    let (task, _, parent) = setup(Modifiable::Yes, Distributable::Yes);
    let foreign = Task::new(2);
    {
        let _serial = foreign.pend.lock();
        assert_eq!(
            ledger::prepare_reservation(&foreign, &parent, id(1), PageCount::new(1).unwrap()).err(),
            Some(PendFail::Denied)
        );
    }
    let _serial = task.pend.lock();
    parent
        .narrow(Limits {
            operations: Operations::Nest(NestRights::INSPECT),
            ..parent.authority().limits()
        })
        .unwrap();
    assert_eq!(
        ledger::prepare_reservation(&task, &parent, id(1), PageCount::new(1).unwrap()).err(),
        Some(PendFail::Denied)
    );
    parent.invalidate();
    assert_eq!(
        ledger::prepare_reservation(&task, &parent, id(1), PageCount::new(1).unwrap()).err(),
        Some(PendFail::Dead)
    );
    assert!(task.pend.heirs.lock().is_empty());
    assert_eq!(task.pend.entries.lock().len(), 1);
}

#[test]
fn unpublished_mapping_drop_cancels_the_real_ledger_and_heir_reservations() {
    let (task, _, parent) = setup(Modifiable::Yes, Distributable::Yes);
    let _serial = task.pend.lock();
    let prepared =
        ledger::prepare_reservation(&task, &parent, id(1), PageCount::new(8).unwrap()).unwrap();
    let token = prepared.entry().token();
    let weak = Arc::downgrade(prepared.entry());
    drop(prepared);
    assert!(weak.upgrade().is_none());
    assert!(task.pend.entries.lock().get(token).is_none());
    assert_eq!(task.pend.entries.lock().len(), 1);
    assert!(task.pend.heirs.lock().is_empty());
    let retry =
        ledger::prepare_reservation(&task, &parent, id(1), PageCount::new(8).unwrap()).unwrap();
    assert_ne!(retry.entry().token(), token);
    drop(retry);
    assert!(parent.alive());
}

#[test]
fn mapping_preparation_allocation_failures_leave_no_hidden_heir_or_registration() {
    let mut failed = 0;
    let mut prepared = 0;
    for remaining in 0..12 {
        let (task, _, parent) = setup(Modifiable::Yes, Distributable::Yes);
        let _serial = task.pend.lock();
        let result = allocation::fail_after(remaining, || {
            ledger::prepare_reservation(&task, &parent, id(1), PageCount::new(8).unwrap())
        });
        match result {
            Err(PendFail::OutOfMemory) => failed += 1,
            Ok(pending) => {
                prepared += 1;
                drop(pending);
            }
            Err(error) => panic!("unexpected preparation failure: {error:?}"),
        }
        assert!(task.pend.heirs.lock().is_empty());
        assert_eq!(task.pend.entries.lock().len(), 1);
        assert!(parent.alive());
        let retry =
            ledger::prepare_reservation(&task, &parent, id(1), PageCount::new(8).unwrap()).unwrap();
        drop(retry);
    }
    assert!(failed != 0 && prepared != 0);
}

#[test]
fn simultaneous_mapping_preparations_reserve_every_heir_publication() {
    let (task, nest, parent) = setup(Modifiable::Yes, Distributable::Yes);
    let _serial = task.pend.lock();
    let pending: Vec<_> = (1..=8)
        .map(|raw| {
            ledger::prepare_reservation(&task, &parent, id(raw), PageCount::new(1).unwrap())
                .unwrap()
        })
        .collect();
    assert!(task.pend.heirs.lock().is_empty());
    for raw in 1..=8 {
        nest.reservations
            .lock()
            .unwrap()
            .push((id(raw), PendStatus::Active));
    }
    let _commit = basis::commit();
    allocation::fail_after(0, || {
        for mapping in pending {
            mapping.publish();
        }
    });
    assert_eq!(task.pend.heirs.lock().len(), 8);
    assert_eq!(task.pend.entries.lock().len(), 9);
}

#[test]
fn ordinary_derivation_preserves_mapping_identity_and_cannot_use_creation_exception() {
    let (task, nest, whole) = setup(Modifiable::Yes, Distributable::Yes);
    let _serial = task.pend.lock();
    let prepared =
        ledger::prepare_reservation(&task, &whole, id(1), PageCount::new(8).unwrap()).unwrap();
    let root = prepared.entry().clone();
    nest.reservations
        .lock()
        .unwrap()
        .push((id(1), PendStatus::Active));
    prepared.publish();
    let limits = Limits {
        operations: Operations::Nest(NestRights::INSPECT),
        modifiable: Modifiable::No,
        distributable: Distributable::No,
        scope: map(1, 2, 2),
    };
    assert_eq!(
        whole.vest_scope(Mark::NONE, limits, false).err(),
        Some(PendFail::Denied)
    );
    for scope in [
        map(2, 2, 2),
        map(1, 7, 2),
        PendScope::Nest(NestScope::Whole),
    ] {
        assert_eq!(
            root.vest_scope(Mark::NONE, Limits { scope, ..limits }, false)
                .err(),
            Some(PendFail::Denied)
        );
    }
    let child = root.vest_scope(Mark::NONE, limits, false).unwrap();
    assert_eq!(child.scope(), limits.scope);
    assert!(child.target().same(root.target()));
    assert_eq!(child.sire(), Some(root.token()));
    assert_eq!(
        child.vest_scope(Mark::NONE, limits, false).err(),
        Some(PendFail::Denied)
    );
    root.invalidate();
    assert!(!child.authority().alive());
}

#[test]
fn namespace_and_mapping_closure_reject_authority_without_losing_observation_identity() {
    let (task, nest, whole) = setup(Modifiable::Yes, Distributable::Yes);
    let serial = task.pend.lock();
    let pending =
        ledger::prepare_reservation(&task, &whole, id(1), PageCount::new(8).unwrap()).unwrap();
    let entry = pending.entry().clone();
    nest.reservations
        .lock()
        .unwrap()
        .push((id(1), PendStatus::Active));
    pending.publish();
    let identity = entry.target().identity();
    nest.reservations.lock().unwrap()[0].1 = PendStatus::Retiring;
    assert!(!entry.alive());
    assert_eq!(entry.status(), PendStatus::Retiring);
    assert_eq!(entry.target().identity(), identity);
    nest.reservations.lock().unwrap()[0].1 = PendStatus::Dead;
    assert_eq!(entry.status(), PendStatus::Dead);
    nest.open.store(false, Ordering::Release);
    assert!(!whole.alive());
    assert_eq!(whole.status(), PendStatus::Retiring);
    assert_eq!(
        ledger::prepare_reservation(&task, &whole, id(2), PageCount::new(1).unwrap())
            .err()
            .map(|e| e),
        Some(PendFail::Dead)
    );
    drop(serial);
    assert_eq!(ledger::vestor(&entry), Some(TaskId::new(1)));
    assert_eq!(
        super::branch::accord(
            &task,
            whole.token(),
            &Arc::downgrade(&task),
            whole.authority().limits(),
            Mark::NONE,
            super::Window::Parent(PendRange::Inherit),
            None,
        ),
        Err(PendFail::Dead)
    );
    nest.closed.store(true, Ordering::Release);
    assert_eq!(whole.status(), PendStatus::Dead);
}
