use crate::{
    cell::{Owner, Page},
    index::PageIndex,
    scope::Scope,
    slot::{self, Slot},
    support::Pool,
};
use std::sync::Arc;

#[test]
fn retirement_outlives_the_source_slot_until_explicit_settlement() {
    let pool = Pool::new(0);
    let slots = slot::admit(
        1,
        vec![(
            PageIndex::ZERO,
            Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap(),
        )],
    )
    .unwrap();
    let Slot::Resident(cell) = &slots[0].1 else {
        panic!("resident input")
    };
    let mut retained = cell.prepare_hardware(false).unwrap();
    let identity = cell.id();
    retained.publish();
    let retired = retained.retire();
    let telemetry = crate::cell::statistics::snapshot();
    assert!(telemetry.cpu >= 4096 && telemetry.retiring >= 4096);
    drop(slots);
    assert_eq!(retired.id(), identity);
    assert_eq!(pool.gives(), 0);
    assert!(pool.page().is_err());

    // SAFETY: this host test published no actual hardware access. It exercises
    // the real bookkeeping; it does not establish hardware fence correctness.
    unsafe { retired.settle() };
    assert_eq!(pool.gives(), 1);
    let next = Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap();
    assert_ne!(next.id(), identity);
}

#[test]
fn non_global_updates_cannot_shrink_a_pending_global_scope() {
    let mut scope = Scope::AddressSpace;
    for global in [false, true, false, false] {
        scope = scope.include(global);
    }
    assert_eq!(scope, Scope::Global);
    assert_eq!(Scope::AddressSpace.include(false), Scope::AddressSpace);
}

#[test]
fn publication_cannot_complete_or_retire_an_unpublished_preparation() {
    use super::memo::Publication;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let mut prepared = Publication::prepare(&page, false, None).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| prepared.complete())).is_err());
    assert!(matches!(prepared.phase(), crate::memo::Phase::Prepared));
    drop(prepared);
    assert!(page.allows_return());
    let prepared = Publication::prepare(&page, false, None).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| prepared.retire())).is_err());
    assert!(page.allows_return());
    let mut published = Publication::prepare(&page, false, None).unwrap();
    published.publish();
    assert!(catch_unwind(AssertUnwindSafe(|| published.publish())).is_err());
    published.complete();
    // No hardware mapping exists in this host bookkeeping test.
    unsafe { published.retire().settle() };
    assert!(page.allows_return());
}
