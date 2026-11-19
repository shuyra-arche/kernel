//! Cell-owned Page gates and non-clonable receipts.
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
