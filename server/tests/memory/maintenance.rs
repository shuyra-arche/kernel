//! Real Page gates, non-clonable receipts and Flat commits. No authority or
//! physical hardware proof is supplied by this harness.
use crate::{
    cell::{Block, Owner, Page, RetainError},
    support::Pool,
};
use std::sync::Arc;

fn page() -> Arc<Page> {
    Arc::new(Page::from_internal(Owner::Kernel, Pool::new(0).page().unwrap()).unwrap())
}
#[test]
fn maintenance_registration_bypasses_only_its_exact_physical_block() {
    let page = page();
    let change = page.begin().unwrap();
    let owner = Block::new(change.clone()).unwrap();
    let permit = owner.permit().unwrap();
    assert!(matches!(
        page.prepare_hardware(false),
        Err(RetainError::Busy)
    ));
    assert!(matches!(change.hardware(false), Err(RetainError::Busy)));
    let mut first = permit.hardware(true).unwrap();
    let mut second = permit.hardware(true).unwrap();
    first.publish();
    second.publish();
    assert_eq!((page.obligations(), page.maintenance_obligations()), (2, 2));
    let other = Block::new(change.clone()).unwrap();
    assert!(matches!(permit.hardware(false), Err(RetainError::Busy)));
    assert!(matches!(other.permit(), Err(RetainError::Busy)));
    assert!(!other.settled());
    // SAFETY: exact modeled receipts; no actual PTE was published on the host.
    unsafe { first.retire().settle() };
    assert_eq!((page.obligations(), page.maintenance_obligations()), (1, 1));
    assert!(!owner.settled());
    unsafe { second.retire().settle() };
    assert!(owner.settled());
    drop(other);
    let refreshed = owner.permit().unwrap();
    drop(refreshed.hardware(false).unwrap());
    drop(owner);
    let replacement = Block::new(change.clone()).unwrap();
    assert!(matches!(permit.hardware(false), Err(RetainError::Busy))); // old physical owner
    let next = replacement.permit().unwrap();
    drop(next.hardware(false).unwrap());
    assert_eq!(page.maintenance_obligations(), 0);
}
#[test]
fn external_receipts_discharge_individually_and_preserve_hardware_exclusion() {
    let page = page();
    let change = page.begin().unwrap();
    let block = Block::new(change.clone()).unwrap();
    let permit = block.permit().unwrap();
    let mut first = permit.external(false).unwrap();
    let mut second = permit.external(false).unwrap();
    first.publish();
    second.publish();
    assert!(matches!(permit.hardware(true), Err(RetainError::Borrowed)));
    assert!(matches!(permit.external(true), Err(RetainError::Borrowed)));
    unsafe { first.retire().settle() }; // modeled exact receipt only
    assert_eq!((page.obligations(), page.maintenance_obligations()), (1, 1));
    assert!(matches!(permit.external(true), Err(RetainError::Borrowed)));
    unsafe { second.retire().settle() };
    let writer = permit.external(true).unwrap();
    assert!(matches!(permit.hardware(false), Err(RetainError::Borrowed)));
    drop(writer); // cancellation before hardware visibility
    assert_eq!((page.obligations(), page.maintenance_obligations()), (0, 0));
    assert!(block.settled());
}
#[test]
fn lost_maintenance_completion_remains_counted_and_quarantined() {
    let page = page();
    let change = page.begin().unwrap();
    let block = Block::new(change).unwrap();
    let permit = block.permit().unwrap();
    let mut lost = permit.hardware(false).unwrap();
    lost.publish();
    drop(lost.retire());
    assert_eq!((page.obligations(), page.maintenance_obligations()), (1, 1));
    assert!(page.quarantined());
    assert!(!block.settled());
    assert!(matches!(
        permit.hardware(false),
        Err(RetainError::Quarantined)
    ));
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
fn maintenance_count_exhaustion_cancels_without_wrapping_other_counts() {
    let page = page();
    let block = Block::new(page.begin().unwrap()).unwrap();
    let permit = block.permit().unwrap();
    let mut prepared = Vec::new();
    loop {
        match permit.hardware(false) {
            Ok(receipt) => prepared.push(receipt),
            Err(RetainError::Exhausted) => break,
            Err(error) => panic!("unexpected admission failure: {error:?}"),
        }
        assert!(prepared.len() <= 65535);
    }
    let count = prepared.len() as u64;
    assert!(count > 1);
    assert_eq!(
        (page.obligations(), page.maintenance_obligations()),
        (count, count)
    );
    // A different hardware kind cannot overflow the shared maintenance count.
    assert!(matches!(
        permit.external(false),
        Err(RetainError::Exhausted)
    ));
    drop(prepared.pop());
    let external = permit.external(false).unwrap();
    assert_eq!(page.obligations(), count);
    drop(prepared);
    assert_eq!((page.obligations(), page.maintenance_obligations()), (1, 1));
    drop(external);
    assert!(block.settled());
    assert_eq!(page.maintenance_obligations(), 0);
}

#[test]
fn arriving_block_and_registration_share_one_atomic_gate() {
    use std::sync::Barrier;
    for _ in 0..32 {
        let page = page();
        let change = page.begin().unwrap();
        let owner = Block::new(change.clone()).unwrap();
        let permit = owner.permit().unwrap();
        let start = Barrier::new(2);
        std::thread::scope(|threads| {
            let pending = threads.spawn(|| {
                start.wait();
                permit.hardware(false)
            });
            start.wait();
            let other = Block::new(change.clone()).unwrap();
            match pending.join().unwrap() {
                Ok(receipt) => {
                    assert!(!other.settled());
                    assert_eq!(page.maintenance_obligations(), 1);
                    drop(receipt); // prepared only, never hardware-visible
                }
                Err(RetainError::Busy) => assert!(other.settled()),
                Err(error) => panic!("unexpected admission failure: {error:?}"),
            }
            assert!(matches!(permit.hardware(false), Err(RetainError::Busy)));
            assert!(other.settled());
        });
    }
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
