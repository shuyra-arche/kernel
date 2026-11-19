use super::{
    cell::{Owner, Page, RetainError},
    support::Pool,
};
use std::sync::{Arc, Barrier};

#[test]
fn completed_gate_metadata_prunes_without_allocating_or_delaying_page_return() {
    let pools: Vec<_> = (0..41).map(|_| Pool::new(0)).collect();
    let pages: Vec<_> = pools
        .iter()
        .map(|pool| Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap()))
        .collect();
    let gates: Vec<_> = pages
        .iter()
        .map(|page| super::changes::acquire(page).unwrap())
        .collect();
    drop(gates);
    // Another concurrent fixture may already have admitted the coalesced bit;
    // either way the real registry retains its pruning work under its lock.
    super::changes::completed();
    let mut turns = 0;
    while super::changes::pending() {
        super::allocation::fail_after(0, super::changes::prune);
        turns += 1;
        assert!(turns < 4096);
    }
    assert!(pages.iter().all(|page| page.allows_return()));
    drop(pages);
    assert!(pools.iter().all(|pool| pool.gives() == 1));
}

#[test]
fn receipt_closes_admission_until_the_last_inflight_holder_finishes() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let receipt = page.begin().unwrap();
    assert!(page.changing());
    assert_eq!(page.begin().unwrap_err(), RetainError::Busy);
    assert_eq!(page.prepare_hardware(false).unwrap_err(), RetainError::Busy);
    assert_eq!(page.prepare_external(false).unwrap_err(), RetainError::Busy);
    let inflight = receipt.clone();
    drop(receipt); // caller ends/exits while an admitted operation still holds it
    assert!(page.changing());
    drop(inflight);
    assert!(!page.changing());
    assert!(super::cell::reclaim::change::take_completion());
    let next = page.begin().unwrap();
    assert!(Arc::ptr_eq(next.page(), &page));
    drop(next);
    drop(page);
    assert_eq!(pool.gives(), 1);
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

#[test]
fn persistent_freezes_block_publication_through_another_logical_position() {
    use super::cell::Freeze;
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let change = page.begin().unwrap();
    let original = Freeze::new(change.clone(), None).unwrap();
    assert!(page.frozen());
    let other = Freeze::new(change.clone(), None).unwrap();
    assert_eq!(change.hardware(true).unwrap_err(), RetainError::Busy);
    assert_eq!(change.external(true).unwrap_err(), RetainError::Busy);
    drop(original);
    let mut writer = change.hardware(true).unwrap();
    writer.publish();
    assert!(!page.frozen());
    unsafe { writer.retire().settle() };
    assert!(page.frozen());
    drop(other);
    drop(change);
    assert!(!page.frozen());
    drop(page);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn freeze_waits_for_other_writers_even_after_their_withdrawal_begins() {
    use super::cell::Freeze;
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let reader = page.prepare_hardware(false).unwrap();
    let mut target = page.prepare_hardware(true).unwrap();
    let mut alias = page.prepare_hardware(true).unwrap();
    target.publish();
    alias.publish();
    let change = page.begin().unwrap();
    let alias = alias.retire();
    assert!(matches!(
        Freeze::new(change.clone(), Some(true)),
        Err(RetainError::Busy)
    ));
    assert!(!page.frozen());
    // SAFETY: host tests install no hardware mappings.
    unsafe { alias.settle() };
    let freeze = Freeze::new(change.clone(), Some(true)).unwrap();
    assert!(target.writable());
    assert!(!page.frozen());
    unsafe { target.retire().settle() };
    assert!(page.frozen());
    assert_eq!(change.external(true).unwrap_err(), RetainError::Borrowed);
    drop(reader);
    drop(freeze);
    drop(change);
    assert!(page.allows_return());
}

#[test]
fn window_blocks_cover_existing_changes_and_reopen_only_after_last_owner() {
    let pool = Pool::new(0);
    let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
    let mut publication = page.prepare_hardware(false).unwrap();
    publication.publish();
    let change = page.begin().unwrap();
    let first = super::cell::Block::new(change.clone()).unwrap();
    let second = super::cell::Block::new(change.clone()).unwrap();
    assert!(!first.settled());
    assert_eq!(change.hardware(false).unwrap_err(), RetainError::Busy);
    assert_eq!(change.external(false).unwrap_err(), RetainError::Busy);
    unsafe { publication.retire().settle() };
    assert!(first.settled());
    drop(first);
    assert_eq!(change.hardware(false).unwrap_err(), RetainError::Busy);
    drop(second);
    drop(change.hardware(false).unwrap());
    drop(change);
    assert!(page.allows_return());
    drop(page);
    assert_eq!(pool.gives(), 1);
}

#[test]
fn concurrent_block_and_hardware_preparation_keep_the_winning_obligation() {
    for _ in 0..128 {
        let pool = Pool::new(0);
        let page = Arc::new(Page::from_internal(Owner::Kernel, pool.page().unwrap()).unwrap());
        let change = page.begin().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let (other, gate) = (change.clone(), barrier.clone());
        let publisher = std::thread::spawn(move || {
            gate.wait();
            other.hardware(false)
        });
        barrier.wait();
        let block = super::cell::Block::new(change.clone()).unwrap();
        let responsibility = publisher.join().unwrap();
        match responsibility {
            Ok(receipt) => {
                assert!(!block.settled());
                drop(receipt);
            }
            Err(error) => assert_eq!(error, RetainError::Busy),
        }
        assert!(block.settled());
        assert_eq!(change.hardware(false).unwrap_err(), RetainError::Busy);
        drop(block);
        drop(change);
        drop(page);
        assert_eq!(pool.gives(), 1);
    }
}
