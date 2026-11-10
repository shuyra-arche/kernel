use kernel_api::model::memory::{Snapshot, SnapshotPage, UNKNOWN};
use std::sync::Arc;
fn value(epoch: u64) -> Snapshot {
    Snapshot {
        epoch,
        start_ns: epoch,
        end_ns: epoch,
        capacity: epoch * 3,
        available: epoch,
        resident: epoch * 2,
        cpu_retained: epoch,
        io_retained: 0,
        retiring: 0,
        abandoned: 0,
        accessed: UNKNOWN,
        dirty: UNKNOWN,
    }
}
#[test]
fn memory_snapshot_is_bounded_and_never_mixes_concurrent_epochs() {
    let page = Arc::new(SnapshotPage::new());
    assert!(page.read().is_none());
    let producer = page.clone();
    let writer = std::thread::spawn(move || {
        for epoch in 1..20_000 {
            assert!(producer.publish(value(epoch)));
        }
    });
    for _ in 0..20_000 {
        if let Some(sample) = page.read() {
            assert_eq!(sample, value(sample.epoch));
        }
    }
    writer.join().unwrap();
    assert_eq!(page.read(), Some(value(19_999)));
    assert_eq!(size_of::<SnapshotPage>(), 4096);
    assert_eq!(align_of::<SnapshotPage>(), 4096);
}
#[test]
fn memory_dispatch_requires_independent_share_and_sign_handles() {
    use kernel_api::{
        boot::ledger::{Entry, Name, Page, ResourceKind},
        model::view::ViewKind,
    };
    let pair = Entry::pair(Name::Memory, ViewKind::Share, 5, 7);
    assert!(pair.valid());
    assert!(!Entry::new(Name::Memory, ResourceKind::Sign, 7).valid());
    assert!(!Entry::pair(Name::Memory, ViewKind::Share, 7, 7).valid());
    assert!(Entry::new(Name::Page(Page::Memory), ResourceKind::Share, 5).valid());
    let mut bytes = [0; Entry::LEN];
    pair.store_at(&mut bytes, 0).unwrap();
    assert_eq!(Entry::fetch_at(&bytes, 0).unwrap().0, pair);
}
