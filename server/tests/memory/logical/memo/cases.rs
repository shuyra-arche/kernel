use super::*;
use super::{
    publication::{Phase, Publication},
    source::{Registry, Source},
    state::Memo,
};
use crate::{
    cell::{Owner, Page},
    support::Pool,
};

fn logical() -> (Memo, Arc<Registry>, Arc<RangeBinding>) {
    let registry = Arc::new(Registry(AtomicUsize::new(0)));
    let binding = Arc::new(RangeBinding);
    let mut memo = Memo::vacant();
    memo.associate(1, None, Source(registry.clone()), binding.clone())
        .release();
    (memo, registry, binding)
}
fn page(pool: &'static Pool) -> Arc<Page> {
    Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap())
}
fn publish(memo: &mut Memo, page: &Arc<Page>) {
    let change = page.begin().unwrap();
    memo.begin_publish(memo.generation(), change.clone(), 6)
        .unwrap();
    let mut publication = Publication::prepare(page, true, Some(&change)).unwrap();
    publication.publish();
    publication.complete();
    memo.install(publication);
    memo.finish().release();
}
fn retract(memo: &mut Memo, page: &Arc<Page>) {
    let change = page.begin().unwrap();
    memo.begin_retract(memo.generation(), change, false)
        .unwrap();
    let publication = memo.publication.take().unwrap().retire();
    // SAFETY: the host fixture explicitly supplies the exact completion point;
    // no real MMU/IOMMU or CPU fence is claimed by this state-machine test.
    unsafe {
        publication.settle();
    }
    memo.finish().release();
}

#[test]
fn next_observation_keeps_physical_identity_through_failed_fence_and_retirement() {
    use kernel_api::model::nest::{MemoPhase, NestId, ReservationId};
    let observe = |memo: &Memo, failed| {
        memo.snapshot(
            NestId::new(1).unwrap(),
            ReservationId::from_bytes(&2u64.to_le_bytes()).unwrap(),
            0,
            kernel_api::model::cell::CellLevel::BASE,
            failed,
        )
    };
    let (mut memo, _, _) = logical();
    let associated = observe(&memo, false);
    assert!(associated.valid());
    assert_eq!(associated.phase, MemoPhase::Associated);
    assert_eq!((associated.page, associated.access), (0, 0));
    let pool = Pool::new(0);
    let data = page(pool);
    let id = data.id().get() as u64;
    let change = data.begin().unwrap();
    memo.begin_publish(memo.generation(), change.clone(), 6)
        .unwrap();
    let pending = observe(&memo, false);
    assert!(pending.valid());
    assert_eq!((pending.phase, pending.page), (MemoPhase::Publishing, id));
    assert_eq!(pending.access, 6);
    assert_eq!(observe(&memo, true).phase, MemoPhase::Failed);
    let mut publication = Publication::prepare(&data, true, Some(&change)).unwrap();
    publication.publish();
    publication.complete();
    memo.install(publication);
    memo.finish().release();
    assert_eq!(observe(&memo, false).phase, MemoPhase::Published);
    memo.record_access(2);
    assert_eq!(observe(&memo, false).access, 2);
    memo.begin_retract(memo.generation(), change.clone(), false)
        .unwrap();
    let retired = memo.publication.take().unwrap().retire();
    let pending = observe(&memo, false);
    assert!(pending.valid());
    assert_eq!((pending.phase, pending.page), (MemoPhase::Retiring, id));
    assert_eq!(pending.access, 2);
    drop(data);
    assert_eq!(pool.gives(), 0);
    unsafe {
        retired.settle();
    } // No physical publication in the host fixture.
    memo.finish().release();
    assert_eq!(observe(&memo, false).phase, MemoPhase::Associated);
    assert_eq!(observe(&memo, false).source, associated.source);
    assert_eq!(observe(&memo, false).page, 0);
    drop(change);
    assert_eq!(pool.gives(), 1);
    memo.remove(memo.generation()).unwrap().release();
    assert_eq!(observe(&memo, false).phase, MemoPhase::Empty);
}

#[test]
fn whole_namespace_withdrawal_retains_memo_page_until_its_batch_settles() {
    use kernel_api::model::nest::{MemoPhase, NestId, ReservationId};
    let (mut memo, registry, binding) = logical();
    let pool = Pool::new(0);
    let data = page(pool);
    let id = data.id().get() as u64;
    publish(&mut memo, &data);
    let before = memo.generation();
    let retired = memo.withdraw().unwrap();
    drop(data);
    assert_eq!(memo.generation(), before + 1);
    assert!(memo.matches(&binding));
    assert!(memo.clear().is_err());
    let status = memo.snapshot(
        NestId::new(1).unwrap(),
        ReservationId::from_bytes(&2u64.to_le_bytes()).unwrap(),
        0,
        kernel_api::model::cell::CellLevel::BASE,
        false,
    );
    assert!(status.valid());
    assert_eq!(
        (status.phase, status.page, status.access),
        (MemoPhase::Retiring, id, 6)
    );
    assert_eq!(pool.gives(), 0);
    // SAFETY: the host models the exact batch receipt completion, not a CPU fence.
    unsafe { retired.settle() };
    assert_eq!(pool.gives(), 0); // Memo's descriptive retention still exists.
    memo.finish().release();
    assert_eq!(pool.gives(), 1);
    assert!(memo.page().is_none());
    memo.clear().unwrap().release();
    assert_eq!(registry.0.load(Ordering::SeqCst), 1);
}

#[test]
fn logical_association_survives_retirement_and_resolves_a_different_physical_page() {
    let (mut memo, registry, binding) = logical();
    assert!(memo.accepts_logical());
    assert!(memo.page().is_none());
    let first_pool = Pool::new(0);
    let first = page(first_pool);
    let first_id = first.id();
    publish(&mut memo, &first);
    assert_eq!(memo.page().unwrap().id(), first_id);
    retract(&mut memo, &first);
    assert!(memo.page().is_none());
    assert!(memo.matches(&binding));
    assert_eq!(registry.0.load(Ordering::SeqCst), 0);
    drop(first);
    assert_eq!(first_pool.gives(), 1);
    let second_pool = Pool::new(0);
    let second = page(second_pool);
    assert_ne!(second.id(), first_id);
    publish(&mut memo, &second);
    assert_eq!(memo.page().unwrap().id(), second.id());
    retract(&mut memo, &second);
    drop(second);
    memo.remove(memo.generation()).unwrap().release();
    assert_eq!(registry.0.load(Ordering::SeqCst), 1);
    assert_eq!(second_pool.gives(), 1);
}

#[test]
fn rejected_reassociation_preserves_binding_generation_and_publication() {
    let (mut memo, registry, binding) = logical();
    let pool = Pool::new(0);
    let page = page(pool);
    let generation = memo.generation();
    assert!(memo.can_associate(generation - 1).is_err());
    publish(&mut memo, &page);
    assert!(memo.can_associate(memo.generation()).is_err());
    assert!(memo.matches(&binding));
    assert_eq!(memo.publication().unwrap().cell().id(), page.id());
    assert_eq!(registry.0.load(Ordering::SeqCst), 0);
    retract(&mut memo, &page);
    let next_registry = Arc::new(Registry(AtomicUsize::new(0)));
    let next_binding = Arc::new(RangeBinding);
    let before = memo.generation();
    memo.associate(
        before,
        None,
        Source(next_registry.clone()),
        next_binding.clone(),
    )
    .release();
    assert_eq!(memo.generation(), before + 1);
    assert!(memo.matches(&next_binding));
    assert!(!memo.matches(&binding));
    assert_eq!(registry.0.load(Ordering::SeqCst), 1);
    memo.clear().unwrap().release();
    drop(page);
    assert_eq!(next_registry.0.load(Ordering::SeqCst), 1);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn retiring_one_logical_writer_does_not_wait_for_or_clear_the_other_writer() {
    let (mut first, _, _) = logical();
    let (mut second, _, _) = logical();
    let pool = Pool::new(0);
    let page = page(pool);
    publish(&mut first, &page);
    publish(&mut second, &page);
    retract(&mut first, &page);
    assert!(first.page().is_none());
    assert_eq!(second.publication().unwrap().phase(), Phase::Mapped);
    assert!(!page.allows_return());
    retract(&mut second, &page);
    assert!(page.allows_return());
    drop(page);
    assert_eq!(pool.gives(), 1);
    first.clear().unwrap().release();
    second.clear().unwrap().release();
}

#[test]
fn legacy_capture_and_retraction_freeze_remain_compatible() {
    let registry = Arc::new(Registry(AtomicUsize::new(0)));
    let pool = Pool::new(0);
    let page = page(pool);
    let mut memo = Memo::vacant();
    assert!(memo.accepts_logical());
    memo.associate(
        1,
        Some(page.clone()),
        Source(registry.clone()),
        Arc::new(RangeBinding),
    )
    .release();
    assert!(!memo.accepts_logical());
    publish(&mut memo, &page);
    retract(&mut memo, &page);
    assert!(memo.page().is_some());
    assert!(memo.frozen());
    drop(page);
    assert_eq!(pool.gives(), 0);
    memo.clear().unwrap().release();
    assert_eq!(pool.gives(), 1);
}

#[test]
fn holding_move_and_restore_preserve_logical_binding_and_page_identity() {
    use crate::{index::PageIndex, residency, state::residency::Error};
    use kernel_api::model::flat::SlotVersion;

    let (mut memo, registry, binding) = logical();
    let mut source = residency::fresh();
    let mut target = residency::fresh();
    let pool = Pool::new(0);
    let (initial, id) = source
        .occupy(PageIndex::ZERO, SlotVersion::INITIAL, residency::page(pool))
        .unwrap();
    let observer = residency::resident(&source);
    // Selection is explicit in this host fixture. Live Source authorization and
    // entry dispatch are not simulated by this combined storage/Memo test.
    publish(&mut memo, &observer);
    assert_eq!(
        source.move_to(
            PageIndex::ZERO,
            initial,
            &mut target,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
        ),
        Err(Error::Busy)
    );
    assert_eq!(source.revision.at(PageIndex::ZERO), initial);
    assert_eq!(target.revision.at(PageIndex::ZERO), SlotVersion::INITIAL);
    assert_eq!(memo.publication().unwrap().cell().id(), id);
    retract(&mut memo, &observer);
    drop(observer);
    let (missing, moved, actual) = source
        .move_to(
            PageIndex::ZERO,
            initial,
            &mut target,
            PageIndex::ZERO,
            SlotVersion::INITIAL,
        )
        .unwrap();
    assert_eq!(actual, id);
    assert!(memo.page().is_none());
    assert!(memo.matches(&binding));
    assert_eq!(registry.0.load(Ordering::SeqCst), 0);
    let rejected = Pool::new(0);
    assert_eq!(
        source.occupy(PageIndex::ZERO, missing, residency::page(rejected)),
        Err(Error::Empty)
    );
    assert_eq!(rejected.gives(), 1);
    let (_, restored, actual) = target
        .move_to(
            PageIndex::ZERO,
            moved,
            &mut source,
            PageIndex::ZERO,
            missing,
        )
        .unwrap();
    assert_eq!(actual, id);
    let observer = residency::resident(&source);
    publish(&mut memo, &observer);
    retract(&mut memo, &observer);
    drop(observer);
    drop(source.vacate(PageIndex::ZERO, restored, None).unwrap().1);
    assert_eq!(pool.gives(), 1);
    assert!(memo.matches(&binding));
    memo.clear().unwrap().release();
    assert_eq!(registry.0.load(Ordering::SeqCst), 1);
}

#[test]
fn unfinished_publication_retains_exact_page_and_prevents_replacement() {
    let (mut memo, registry, binding) = logical();
    let pool = Pool::new(0);
    let page = page(pool);
    let change = page.begin().unwrap();
    memo.begin_publish(memo.generation(), change.clone(), 6)
        .unwrap();
    let mut publication = Publication::prepare(&page, true, Some(&change)).unwrap();
    publication.publish();
    memo.install(publication);
    drop(page);
    // No visibility proof was supplied. A finish attempt cannot turn this into
    // a usable publication or release the retained Page/association.
    memo.finish().release();
    assert!(memo.in_flight());
    assert!(memo.can_associate(memo.generation()).is_err());
    assert!(memo.clear().is_err());
    assert!(memo.matches(&binding));
    assert_eq!(registry.0.load(Ordering::SeqCst), 0);
    assert_eq!(pool.gives(), 0);
    // The original in-flight Change still owns closed admission. Cleanup uses
    // that permit instead of attempting to open a competing Page change.
    memo.begin_retract(memo.generation(), change, false)
        .unwrap();
    let publication = memo.publication.take().unwrap().retire();
    // SAFETY: exact host-model retirement point, not physical fence evidence.
    unsafe { publication.settle() };
    memo.finish().release();
    assert_eq!(pool.gives(), 1);
    assert!(memo.page().is_none());
    memo.clear().unwrap().release();
    assert_eq!(registry.0.load(Ordering::SeqCst), 1);
}
