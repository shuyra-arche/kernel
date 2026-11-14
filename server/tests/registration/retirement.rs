//! Real Pend transactions; Namespace spans and hardware completions are scripted.
use super::{
    Window, basis, branch, ledger,
    reservation::{id, map, setup},
    support::{Nest, Task},
};
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::{
        cell::CellLevel,
        identity::Mark,
        modifiable::Modifiable::{No, Yes},
        pend::{
            Distributable, Limits, NestRights, NestScope, Operations, PendRange, PendScope,
            PendStatus,
        },
    },
};
use std::sync::{Arc, atomic::Ordering};

fn reserve(
    task: &Arc<Task>,
    nest: &Nest,
    whole: &Arc<ledger::PendEntry>,
    count: usize,
    level: CellLevel,
) -> Arc<ledger::PendEntry> {
    let _serial = task.pend.lock();
    let count = PageCount::new(count).unwrap();
    let prepared = ledger::prepare_reservation(task, whole, id(1), count).unwrap();
    let root = prepared.entry().clone();
    let _commit = basis::commit();
    nest.install(id(1), count, level, root.authority());
    prepared.publish();
    root
}

fn child(
    task: &Arc<Task>,
    target: &Arc<Task>,
    parent: &ledger::PendEntry,
    range: PendRange,
) -> Arc<ledger::PendEntry> {
    let token = branch::accord(
        task,
        parent.token(),
        &Arc::downgrade(target),
        Limits {
            scope: PendScope::Nest(NestScope::Reservation { id: id(1), range }),
            ..parent.authority().limits()
        },
        Mark::NONE,
        Window::Parent(range),
        None,
    )
    .unwrap();
    ledger::locate(target, kernel_api::model::identity::PendId::mint(token)).unwrap()
}

#[test]
fn returned_reservation_roots_reap_scripted_metadata_after_real_authority_settlement() {
    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    for raw in 1..=64 {
        let root = {
            let _serial = task.pend.lock();
            let count = PageCount::new(1).unwrap();
            let prepared = ledger::prepare_reservation(&task, &whole, id(raw), count).unwrap();
            let root = prepared.entry().clone();
            let _commit = basis::commit();
            nest.install(id(raw), count, CellLevel::BASE, root.authority());
            prepared.publish();
            root
        };
        nest.retire_reservation(raw);
        assert!(nest.reservation(id(raw)).is_some());
        ledger::return_(&task, root.token()).unwrap();
        assert_eq!(root.authority().status(), PendStatus::Dead);
        assert!(ledger::locate(&task, root.token()).is_none());
        assert!(nest.reservation(id(raw)).is_none());
        assert!(nest.reservations.lock().unwrap().is_empty());
        assert!(task.pend.heirs.lock().is_empty());
        assert!(whole.alive());
    }
}

#[test]
fn mapping_owner_is_nest_creator_and_lender_is_the_actual_reserving_parent() {
    let (creator, nest, whole) = setup(Yes, Distributable::Yes);
    let worker = Task::new(2);
    let token = branch::accord(
        &creator,
        whole.token(),
        &Arc::downgrade(&worker),
        whole.authority().limits(),
        Mark::NONE,
        Window::Parent(PendRange::Inherit),
        None,
    )
    .unwrap();
    let grant = ledger::locate(&worker, kernel_api::model::identity::PendId::mint(token)).unwrap();
    let mapping = reserve(&worker, &nest, &grant, 8, CellLevel::BASE);
    assert_eq!(mapping.target().owner(), creator.ident.id);
    assert_eq!(ledger::vestor(&mapping), Some(worker.ident.id));
    assert_eq!(ledger::vestor(&grant), Some(creator.ident.id));
    whole.invalidate();
    assert!(!mapping.alive());
}

#[test]
fn relative_accord_and_narrow_publish_only_resolved_absolute_scopes() {
    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    let root = reserve(&task, &nest, &whole, 8, CellLevel::BASE);
    let first = child(&task, &task, &root, PendRange::pages(2, 4).unwrap());
    let nested = child(&task, &task, &first, PendRange::pages(1, 2).unwrap());
    assert_eq!(first.scope(), map(1, 2, 4));
    assert_eq!(nested.scope(), map(1, 3, 2));
    ledger::return_(&task, nested.token()).unwrap();
    branch::reduce(
        &task,
        first.token(),
        Limits {
            scope: map(1, 1, 2),
            ..first.authority().limits()
        },
    )
    .unwrap();
    assert_eq!(first.scope(), map(1, 3, 2));
    assert_eq!(first.authority().limits().scope, map(1, 3, 2));
}

#[test]
fn creating_whole_cannot_narrow_until_its_mapping_heir_is_removed() {
    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    let root = reserve(&task, &nest, &whole, 8, CellLevel::BASE);
    let original = whole.authority().limits();
    let limits = Limits {
        operations: Operations::Nest(NestRights::INSPECT),
        ..original
    };
    assert_eq!(
        branch::reduce(&task, whole.token(), limits),
        Err(PendFail::Busy)
    );
    assert_eq!(whole.authority().limits(), original);
    ledger::return_(&task, root.token()).unwrap();
    assert!(task.pend.heirs.lock().is_empty());
    branch::reduce(&task, whole.token(), limits).unwrap();
    assert_eq!(whole.authority().limits(), limits);
    assert!(nest.is_open());
}

#[test]
fn narrowed_reservation_root_return_retires_the_original_span_despite_its_sire() {
    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    let root = reserve(&task, &nest, &whole, 8, CellLevel::BASE);
    assert_eq!(root.sire(), Some(whole.token()));
    branch::reduce(
        &task,
        root.token(),
        Limits {
            operations: Operations::Nest(NestRights::INSPECT),
            modifiable: No,
            distributable: Distributable::No,
            scope: map(1, 2, 2),
        },
    )
    .unwrap();
    assert_eq!(root.scope(), map(1, 2, 2));
    ledger::return_(&task, root.token()).unwrap();
    assert_eq!(
        *nest.released.lock().unwrap(),
        [(id(1), PageCount::new(8).unwrap())]
    );
    assert!(ledger::locate(&task, root.token()).is_none());
    assert!(whole.alive());
}

#[test]
fn child_return_and_remote_revoke_withdraw_only_their_exact_publisher_routes() {
    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    let remote = Task::new(2);
    let root = reserve(&task, &nest, &whole, 8, CellLevel::BASE);
    let first = child(&task, &task, &root, PendRange::pages(1, 2).unwrap());
    let sibling = child(&task, &remote, &root, PendRange::pages(4, 2).unwrap());
    nest.publish(id(1), first.authority());
    nest.publish(id(1), sibling.authority());
    ledger::return_(&task, first.token()).unwrap();
    assert!(!first.authority().external_pending());
    assert!(sibling.alive() && sibling.authority().external_pending());
    assert!(root.alive() && nest.released.lock().unwrap().is_empty());
    {
        let withdrawn = nest.withdrawn.lock().unwrap();
        assert_eq!(withdrawn.len(), 1);
        assert_eq!(
            withdrawn[0].1,
            (PageOffset::new(1), PageCount::new(2).unwrap())
        );
        assert!(withdrawn[0].2.ptr_eq(&Arc::downgrade(first.authority())));
    }
    ledger::revoke(&task, &Arc::downgrade(&remote), sibling.token()).unwrap();
    assert!(!sibling.authority().external_pending());
    assert_eq!(nest.withdrawn.lock().unwrap().len(), 2);
    assert!(root.alive() && nest.released.lock().unwrap().is_empty());
    assert_eq!(task.pend.heirs.lock().len(), 1); // Whole -> mapping root remains.
}

#[test]
fn narrow_withdraws_publication_before_committing_and_busy_preserves_limits() {
    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    let root = reserve(&task, &nest, &whole, 8, CellLevel::BASE);
    let original = root.authority().limits();
    let request = Limits {
        operations: Operations::Nest(NestRights::INSPECT),
        modifiable: No,
        scope: map(1, 2, 2),
        ..original
    };
    nest.publish(id(1), root.authority());
    nest.busy.store(true, Ordering::Release);
    assert_eq!(
        branch::reduce(&task, root.token(), request),
        Err(PendFail::Busy)
    );
    assert_eq!(root.authority().limits(), original);
    assert!(root.authority().external_pending());
    nest.busy.store(false, Ordering::Release);
    branch::reduce(&task, root.token(), request).unwrap();
    assert_eq!(root.authority().limits(), request);
    assert!(!root.authority().external_pending());
    let withdrawn = nest.withdrawn.lock().unwrap();
    assert_eq!(
        withdrawn[0].1,
        (PageOffset::ZERO, PageCount::new(8).unwrap())
    );
}

#[test]
fn gone_mapping_metadata_remains_reachable_for_busy_return_retry() {
    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    let root = reserve(&task, &nest, &whole, 8, CellLevel::BASE);
    nest.reservations.lock().unwrap()[0].1 = PendStatus::Dead;
    assert_eq!(root.status(), PendStatus::Dead);
    assert!(!root.alive());
    nest.busy.store(true, Ordering::Release);
    assert_eq!(ledger::return_(&task, root.token()), Err(PendFail::Busy));
    assert!(ledger::locate(&task, root.token()).is_some());
    nest.busy.store(false, Ordering::Release);
    ledger::return_(&task, root.token()).unwrap();
    assert_eq!(
        *nest.released.lock().unwrap(),
        [(id(1), PageCount::new(8).unwrap())]
    );
}

#[test]
fn only_unadopted_internal_whole_root_return_closes_the_cpu_namespace() {
    for (adopted, external) in [(false, false), (true, false), (false, true)] {
        let (task, nest, whole) = setup(Yes, Distributable::Yes);
        nest.adopted.store(adopted, Ordering::Release);
        nest.external.store(external, Ordering::Release);
        ledger::return_(&task, whole.token()).unwrap();
        assert_eq!(nest.is_closed(), !adopted && !external);
    }
    // External whole-context closure belongs to its exact-root keeper, not cull.
}

#[test]
fn accord_and_narrow_check_complete_cell_and_external_original_span_geometry() {
    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    let root = reserve(&task, &nest, &whole, 1024, CellLevel::new(1).unwrap());
    let original = root.authority().limits();
    assert_eq!(
        branch::reduce(
            &task,
            root.token(),
            Limits {
                scope: map(1, 1, 512),
                ..original
            }
        ),
        Err(PendFail::NotAligned)
    );
    let part = child(&task, &task, &root, PendRange::pages(512, 512).unwrap());
    assert_eq!(part.scope(), map(1, 512, 512));
    ledger::return_(&task, part.token()).unwrap();
    branch::reduce(
        &task,
        root.token(),
        Limits {
            scope: map(1, 512, 512),
            ..original
        },
    )
    .unwrap();
    assert_eq!(root.scope(), map(1, 512, 512));

    let (task, nest, whole) = setup(Yes, Distributable::Yes);
    nest.external.store(true, Ordering::Release);
    let root = reserve(&task, &nest, &whole, 8, CellLevel::BASE);
    assert_eq!(
        branch::reduce(
            &task,
            root.token(),
            Limits {
                scope: map(1, 0, 4),
                ..root.authority().limits()
            }
        ),
        Err(PendFail::NotAligned)
    );
    let full = child(&task, &task, &root, PendRange::Inherit);
    assert_eq!(full.scope(), root.scope());
}
