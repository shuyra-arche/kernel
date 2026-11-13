use super::{allocation, ledger::Table};
use kernel_api::{abi::call::pend::PendFail, model::identity::PendId};
use std::sync::Weak;

#[test]
fn heir_admission_counts_pending_reservations_and_drop_returns_capacity() {
    let table = Table::new(kernel_api::model::identity::TaskId::new(0)).unwrap();
    let parent = PendId::mint(1);
    let mut pending: Vec<_> = (2..1026)
        .map(|raw| {
            table
                .prepare_heir(parent, Weak::new(), PendId::mint(raw))
                .unwrap()
        })
        .collect();
    assert!(table.heirs.lock().is_empty());
    assert_eq!(
        table
            .prepare_heir(parent, Weak::new(), PendId::mint(1026))
            .err(),
        Some(PendFail::OutOfMemory)
    );
    drop(pending.pop());
    let replacement = table
        .prepare_heir(parent, Weak::new(), PendId::mint(1027))
        .unwrap();
    allocation::fail_after(0, || {
        replacement.publish();
        for heir in pending {
            heir.publish();
        }
    });
    assert_eq!(table.heirs.lock().len(), 1024);
    assert_eq!(
        table
            .prepare_heir(parent, Weak::new(), PendId::mint(1028))
            .err(),
        Some(PendFail::OutOfMemory)
    );
}

#[test]
fn closed_ledger_rejects_fresh_heir_preparation() {
    let table = Table::new(kernel_api::model::identity::TaskId::new(0)).unwrap();
    table.close();
    assert_eq!(
        table
            .prepare_heir(PendId::mint(1), Weak::new(), PendId::mint(2))
            .err(),
        Some(PendFail::Dead)
    );
}
