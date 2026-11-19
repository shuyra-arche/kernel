//! Cross-owner Page and Nest Publication bookkeeping; no actual PTE is installed.
use super::{
    cell::{Exposure, Owner, Page, RetainError},
    memo::Publication,
    support::Pool,
};
use std::sync::Arc;

#[test]
fn a_change_preserves_memos_but_cannot_reuse_the_page_until_all_are_withdrawn() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let id = page.id();
    let mut left = Publication::prepare(&page, false, None).unwrap();
    let mut right = Publication::prepare(&page, false, None).unwrap();
    left.publish();
    right.publish();
    left.complete();
    right.complete();
    let change = page.begin().unwrap();
    assert!(matches!(left.phase(), crate::memo::Phase::Mapped));
    assert_eq!(left.id(), id);
    assert_eq!(change.external(true).unwrap_err(), RetainError::Borrowed);
    // SAFETY: host tests publish no hardware; this tests bookkeeping only.
    unsafe { left.retire().settle() };
    assert_eq!(change.external(true).unwrap_err(), RetainError::Borrowed);
    unsafe { right.retire().settle() };
    let mut dma = change.external(true).unwrap();
    dma.publish();
    assert_eq!(page.exposure(), Exposure::External);
    drop(change); // ending the change is not DMA completion
    assert_eq!(
        page.prepare_hardware(true).unwrap_err(),
        RetainError::Borrowed
    );
    let retiring = dma.retire();
    drop(page);
    assert_eq!(pool.gives(), 0);
    unsafe { retiring.settle() };
    assert_eq!(pool.gives(), 1);
}

#[test]
fn fault_supply_uses_the_same_receipt_and_preserves_page_identity() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let change = page.begin().unwrap();
    let mut memo = Publication::prepare(&page, true, Some(&change)).unwrap();
    let id = page.id();
    memo.publish();
    assert!(matches!(memo.phase(), crate::memo::Phase::Publishing));
    memo.complete();
    assert!(matches!(memo.phase(), crate::memo::Phase::Mapped));
    assert_eq!(memo.id(), id);
    drop(change);
    drop(page);
    assert_eq!(pool.gives(), 0);
    let memo = memo.retire();
    assert!(matches!(memo.phase(), crate::memo::Phase::Withdrawing));
    unsafe { memo.settle() };
    assert_eq!(pool.gives(), 1);
}

#[test]
fn swap_out_and_in_preserve_bytes_while_reused_frames_receive_new_identity() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let old = page.id();
    let pa = page.physical_address();
    let mut memo = Publication::prepare(&page, true, None).unwrap();
    memo.publish();
    memo.complete();
    // Simulate payload writes with exclusive CPU responsibility, no real PTE.
    unsafe { page.as_mut_ptr().write_bytes(0x5a, 4096) };
    let change = page.begin().unwrap();
    unsafe { memo.retire().settle() };
    let mut io = change.external(false).unwrap();
    io.publish();
    let mut backing = [0u8; 4096];
    unsafe {
        backing
            .as_mut_ptr()
            .copy_from_nonoverlapping(page.as_mut_ptr(), 4096)
    };
    unsafe { io.retire().settle() };
    drop(change);
    drop(page);
    assert_eq!(pool.gives(), 1);
    let restored = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    assert_eq!(restored.physical_address(), pa);
    assert_ne!(restored.id(), old);
    let change = restored.begin().unwrap();
    let mut io = change.external(true).unwrap();
    io.publish();
    unsafe {
        restored
            .as_mut_ptr()
            .copy_from_nonoverlapping(backing.as_ptr(), 4096)
    };
    unsafe { io.retire().settle() };
    let mut memo = Publication::prepare(&restored, true, Some(&change)).unwrap();
    memo.publish();
    memo.complete();
    drop(change);
    assert!(
        unsafe { std::slice::from_raw_parts(restored.as_mut_ptr(), 4096) }
            .iter()
            .all(|b| *b == 0x5a)
    );
    unsafe { memo.retire().settle() };
    drop(restored);
    assert_eq!(pool.gives(), 2);
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
