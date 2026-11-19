//! Flat staging, provenance and transfer under held Cell gates.
//! Host bookkeeping cannot prove actual hardware quiescence.
use crate::{
    cell::{Block, Owner, Page, RetainError},
    support::Pool,
};
use std::sync::Arc;

fn page() -> Arc<Page> {
    Arc::new(Page::from_internal(Owner::Kernel, Pool::new(0).page().unwrap()).unwrap())
}
#[test]
fn holding_move_preserves_gate_and_staging_metadata_until_return() {
    use crate::{residency::fresh, state::residency::Error};
    use kernel_api::{address::PageIndex, model::flat::SlotVersion};
    let mut from = fresh();
    let mut to = fresh();
    let pool = Pool::new(0);
    let (version, _) = from
        .occupy(
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap(),
        )
        .unwrap();
    let crate::state::FlatSource::Managed(slots) = &from.source else {
        unreachable!()
    };
    let crate::slot::Slot::Resident(page) = &slots[0].1 else {
        unreachable!()
    };
    let page = page.clone();
    let change = crate::changes::acquire(&page).unwrap();
    let block = Block::new(change.clone()).unwrap();
    // Opaque origin metadata fixture; the production Flat commit owns its lifetime.
    let stage = Arc::new(crate::basis::pend::Stage);
    let weak = Arc::downgrade(&stage);
    assert!(matches!(
        from.move_to(
            PageIndex::ZERO,
            version,
            &mut to,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            None
        ),
        Err(Error::Busy)
    ));
    let (missing, present, id) = from
        .move_to_during(
            PageIndex::ZERO,
            version,
            &mut to,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            Some(&change),
            Some(stage),
            None,
        )
        .unwrap();
    assert_eq!(id, page.id());
    assert_eq!(to.staged.len(), 1);
    assert!(weak.upgrade().is_some());
    assert!(matches!(
        page.prepare_hardware(false),
        Err(RetainError::Busy)
    ));
    assert!(matches!(
        to.move_to_during(
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            &mut from,
            PageIndex::ZERO,
            missing,
            Some(&change),
            None,
            None
        ),
        Err(Error::Stale)
    ));
    assert_eq!(to.staged.len(), 1);
    let permit = block.permit().unwrap();
    let mut publication = permit.hardware(true).unwrap();
    publication.publish();
    assert!(matches!(
        to.move_to_during(
            PageIndex::ZERO,
            present,
            &mut from,
            PageIndex::ZERO,
            missing,
            Some(&change),
            None,
            None,
        ),
        Err(Error::Busy)
    ));
    assert_eq!(to.staged.len(), 1);
    assert!(weak.upgrade().is_some());
    // No real PTE exists; only this exact modeled publication is completed.
    unsafe { publication.retire().settle() };
    drop(permit);
    to.move_to_during(
        PageIndex::ZERO,
        present,
        &mut from,
        PageIndex::ZERO,
        missing,
        Some(&change),
        None,
        None,
    )
    .unwrap();
    assert!(to.staged.is_empty());
    assert!(weak.upgrade().is_none());
    drop(block);
    drop(change);
    drop(page);
    drop(from);
    drop(to);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn provenance_moves_atomically_and_survives_maintenance_gate_release() {
    use crate::{
        residency::{fresh, page},
        state::residency::Error,
    };
    use kernel_api::{address::PageIndex, model::flat::SlotVersion};
    let mut from = fresh();
    let mut to = fresh();
    let mut third = fresh();
    let pool = Pool::new(0);
    let (version, id) = from
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
        .unwrap();
    let data = crate::residency::resident(&from);
    let change = crate::changes::acquire(&data).unwrap();
    let block = Block::new(change.clone()).unwrap();
    let origin = Arc::new(crate::basis::pend::Provenance);
    let weak = Arc::downgrade(&origin);
    let (_, stamp, actual) = from
        .move_to_during(
            PageIndex::ZERO,
            version,
            &mut to,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            Some(&change),
            None,
            Some(origin),
        )
        .unwrap();
    assert_eq!(actual, id);
    drop(block);
    drop(change);
    drop(data);
    // Ending maintenance must not remove original-source restrictions.
    assert!(weak.upgrade().is_some());
    let origin = to.provenance[0].1.clone();
    assert_eq!(
        to.move_to(
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            &mut third,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            Some(origin.clone())
        ),
        Err(Error::Stale)
    );
    assert!(Arc::ptr_eq(&to.provenance[0].1, &origin));
    let (_, last, actual) = to
        .move_to(
            PageIndex::ZERO,
            stamp,
            &mut third,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
            Some(origin),
        )
        .unwrap();
    assert_eq!(actual, id);
    assert!(to.provenance.is_empty());
    assert!(weak.upgrade().is_some());
    drop(third.vacate(PageIndex::ZERO, last, None).unwrap().1);
    assert!(third.provenance.is_empty());
    assert!(weak.upgrade().is_none());
    assert_eq!(pool.gives(), 1);
}

#[test]
fn provenance_preparation_failure_preserves_both_holdings_and_old_metadata() {
    use crate::{
        residency::{fresh, page},
        state::residency::Error,
    };
    use kernel_api::{address::PageIndex, model::flat::SlotVersion};
    let mut succeeded = false;
    for budget in 0..32 {
        let mut from = fresh();
        let mut to = fresh();
        let pool = Pool::new(0);
        let (version, id) = from
            .occupy(PageIndex::ZERO, SlotVersion::INITIAL, page(pool))
            .unwrap();
        let original = Arc::new(crate::basis::pend::Provenance);
        from.provenance.push((PageIndex::ZERO, original.clone()));
        let next = Arc::new(crate::basis::pend::Provenance);
        let result = crate::allocation::fail_after(budget, || {
            from.move_to(
                PageIndex::ZERO,
                version,
                &mut to,
                PageIndex::ZERO,
                SlotVersion::INITIAL,
                Some(next.clone()),
            )
        });
        match result {
            Ok((_, _, actual)) => {
                assert_eq!(actual, id);
                assert!(from.provenance.is_empty());
                assert!(Arc::ptr_eq(&to.provenance[0].1, &next));
                succeeded = true;
            }
            Err(Error::OutOfMemory) => {
                assert_eq!(from.revision.at(PageIndex::ZERO), version);
                assert_eq!(to.revision.at(PageIndex::ZERO), SlotVersion::INITIAL);
                assert!(Arc::ptr_eq(&from.provenance[0].1, &original));
                assert!(to.provenance.is_empty());
                assert_eq!(crate::residency::resident(&from).id(), id);
            }
            other => panic!("unexpected transfer result: {other:?}"),
        }
        drop(from);
        drop(to);
        assert_eq!(pool.gives(), 1);
        if succeeded {
            break;
        }
    }
    assert!(succeeded);
}
