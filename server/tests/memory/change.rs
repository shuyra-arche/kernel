use super::{
    cell::{Exposure, Owner, Page, RetainError},
    memo::Memo,
    support::Pool,
};
use std::sync::{Arc, Barrier};

#[test]
fn receipt_closes_admission_until_the_last_inflight_holder_finishes() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let receipt = page.begin().unwrap();
    let old = receipt.id();
    assert!(page.changing());
    assert_eq!(page.begin().unwrap_err(), RetainError::Busy);
    assert_eq!(page.prepare_hardware(false).unwrap_err(), RetainError::Busy);
    assert_eq!(page.prepare_external(false).unwrap_err(), RetainError::Busy);
    let inflight = receipt.clone();
    drop(receipt); // caller ends/exits while an admitted operation still holds it
    assert!(page.changing());
    drop(inflight);
    assert!(!page.changing());
    assert!(super::cell::change::take_completion());
    let next = page.begin().unwrap();
    assert_ne!(old, next.id());
    drop(next);
    drop(page);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn a_change_preserves_memos_but_cannot_reuse_the_page_until_all_are_withdrawn() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let id = page.id();
    let mut left = Memo::prepare(&page, false, None).unwrap();
    let mut right = Memo::prepare(&page, false, None).unwrap();
    left.publish();
    right.publish();
    left.complete();
    right.complete();
    let change = page.begin().unwrap();
    assert!(matches!(left, Memo::Mapped(_)));
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
    let mut memo = Memo::prepare(&page, true, Some(&change)).unwrap();
    let id = page.id();
    memo.publish();
    assert!(matches!(memo, Memo::Publishing(_)));
    memo.complete();
    assert!(matches!(memo, Memo::Mapped(_)));
    assert_eq!(memo.id(), id);
    drop(change);
    drop(page);
    assert_eq!(pool.gives(), 0);
    let memo = memo.retire();
    assert!(matches!(memo, Memo::Withdrawing(_)));
    unsafe { memo.settle() };
    assert_eq!(pool.gives(), 1);
}

#[test]
fn swap_out_and_in_preserve_bytes_while_reused_frames_receive_new_identity() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let old = page.id();
    let pa = page.physical_address();
    let mut memo = Memo::prepare(&page, true, None).unwrap();
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
    let mut memo = Memo::prepare(&restored, true, Some(&change)).unwrap();
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
fn registration_racing_begin_cannot_cross_the_closed_gate() {
    for _ in 0..64 {
        let pool = Pool::new(0);
        let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
        let barrier = Arc::new(Barrier::new(2));
        let other = page.clone();
        let signal = barrier.clone();
        let thread = std::thread::spawn(move || {
            signal.wait();
            other.prepare_external(false)
        });
        barrier.wait();
        let change = page.begin().unwrap();
        let raced = thread.join().unwrap();
        // Either it entered before Begin or was rejected atomically.
        assert!(raced.is_ok() || matches!(raced, Err(RetainError::Busy)));
        assert_eq!(page.prepare_external(false).unwrap_err(), RetainError::Busy);
        drop(raced);
        drop(change);
        drop(page);
        assert_eq!(pool.gives(), 1);
    }
}
