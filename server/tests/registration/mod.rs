#![feature(allocator_ext)]
extern crate alloc;

// Exercise production registries, rollback guards and delegated authority chains.
// Only hardware/scheduling and the unrelated Pend payload are host substitutes;
// the guest suite exercises the full Sign/Pend path through the real Kernel.
#[path = "../name/allocation.rs"]
mod allocation;
#[allow(dead_code)]
#[path = "../../src/basis/unit/control/mod.rs"]
mod control;
#[allow(dead_code)]
#[path = "../../src/basis/pend/ledger/table.rs"]
mod ledger;
#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;
mod support;
use kernel_api::{
    abi::call::unit::UnitFail,
    model::{identity::PendId, unit::ControlKind},
};
use std::sync::Arc;
use support::Task;
mod task {
    pub(crate) use crate::support::Task;
}
use support::{HoldTable, PendEntry};

mod basis {
    pub(crate) use crate::name;
    pub(crate) mod unit {
        pub(crate) mod task {
            pub(crate) use crate::support::Task;
        }
    }
    pub(crate) mod pend {
        pub(crate) fn live(_: &super::unit::task::Task) -> bool {
            true
        }
    }
    pub(crate) fn commit() -> std::sync::MutexGuard<'static, ()> {
        static COMMIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
        COMMIT.lock().unwrap()
    }
}
mod metal {
    pub(crate) mod sync {
        pub(crate) use crate::support::{SpinLock, spin};
    }
}

#[test]
fn unpublished_pend_rolls_back_and_keeps_one_record_on_publish() {
    let table = ledger::Table::new().unwrap();
    let _transaction = table.lock();
    let entry = Arc::new(PendEntry(PendId::mint(1)));
    let failed = (|| -> Result<(), ()> {
        let _registration = table.prepare(entry.clone()).unwrap();
        assert!(table.entries.lock().get(entry.token()).is_none());
        Err(())
    })();
    assert_eq!(failed, Err(()));
    assert_eq!(table.entries.lock().len(), 0);
    let rejected = table
        .prepare(entry.clone())
        .unwrap()
        .commit(|_publish| Err::<(), _>(()));
    assert_eq!(rejected, Err(()));
    let registration = table.prepare(entry.clone()).unwrap();
    // Publication uses reserved memory and cannot fail with allocation disabled.
    allocation::fail_after(0, || registration.publish());
    assert!(Arc::ptr_eq(
        table.entries.lock().get(entry.token()).unwrap(),
        &entry
    ));
    assert_eq!(table.entries.lock().len(), 1);
}

#[test]
fn failed_pend_preparation_and_batch_leave_no_hidden_reservations() {
    for fail in [0, 1] {
        let table = ledger::Table::new().unwrap();
        let entry = Arc::new(PendEntry(PendId::mint(1)));
        assert!(allocation::fail_after(fail, || table.prepare(entry.clone())).is_err());
        table.prepare(entry).unwrap().publish();
        assert_eq!(table.entries.lock().len(), 1);
    }
    let table = ledger::Table::new().unwrap();
    let records: Vec<_> = (1..=8)
        .map(|id| Arc::new(PendEntry(PendId::mint(id))))
        .collect();
    // Simulate an error during a multi-record delivery after several reservations.
    let prepared: Vec<_> = records
        .iter()
        .map(|r| table.prepare(r.clone()).unwrap())
        .collect();
    assert_eq!(table.entries.lock().len(), 0);
    drop(prepared);
    for record in records {
        table.prepare(record).unwrap().publish();
    }
    assert_eq!(table.entries.lock().len(), 8);
}

#[test]
fn control_reuse_does_not_revive_revoked_descendants_or_cross_tables() {
    let a = Task::new(1);
    let b = Task::new(2);
    let c = Task::new(3);
    let root = a.administration.install(ControlKind::Build, true).unwrap();
    assert!(!b.administration.matches(root, ControlKind::Build));
    let child = control::delegate(&a, &b, root, true).unwrap();
    let leaf = control::delegate(&b, &c, child, false).unwrap();
    a.administration
        .revoke(root, &b.administration, child)
        .unwrap();
    assert!(!c.administration.matches(leaf, ControlKind::Build));
    b.administration.release(child).unwrap();
    let replacement = control::delegate(&a, &b, root, true).unwrap();
    assert_ne!(child, replacement);
    assert!(!b.administration.matches(child, ControlKind::Build));
    assert!(b.administration.matches(replacement, ControlKind::Build));
    assert!(!c.administration.allows(ControlKind::Build));
    assert_eq!(
        control::delegate(&c, &a, leaf, false),
        Err(UnitFail::Denied)
    );
    a.administration.clear();
    assert!(!b.administration.allows(ControlKind::Build));
}
