#![feature(allocator_ext)]
extern crate alloc;

// Production authority, scopes, ancestry and ledger preparation run on the host.
// Namespace metadata and hardware notifications are fixtures, not full Kernel enforcement.
#[path = "../name/allocation.rs"]
mod allocation;
#[allow(dead_code)]
#[path = "../../src/basis/pend/authority/mod.rs"]
mod authority;
#[path = "../../src/basis/pend/branch.rs"]
mod branch;
mod carried;
#[allow(dead_code)]
#[path = "../../src/basis/unit/control/mod.rs"]
pub(crate) mod control;
#[path = "../../src/basis/pend/source/epoch.rs"]
mod epoch;
mod exports;
#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/pend/ledger/mod.rs"]
mod ledger;
mod maintenance;
#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;
mod reservation;
#[path = "ledger.rs"]
mod reservations;
mod retirement;
#[allow(dead_code)]
mod support;
#[allow(dead_code)]
#[path = "../../src/basis/pend/target.rs"]
mod target;
#[allow(dead_code)]
#[path = "../../src/basis/pend/window.rs"]
mod window;
use kernel_api::{abi::call::unit::UnitFail, model::unit::ControlKind};
use std::sync::Arc;
use support::Task;
mod task {
    pub(crate) use crate::support::Task;
}
use authority::Pend;
use basis::pend::live;
use ledger::{locate, notify};
use support::{HoldTable, PendEntry};
use window::Window;

#[allow(dead_code)]
mod basis {
    pub(crate) use crate::name;
    pub(crate) mod unit {
        pub(crate) use crate::control;
        pub(crate) mod task {
            pub(crate) use crate::support::Task;
        }
        pub(crate) mod anchor {
            pub(crate) use crate::support::Anchor;
        }
    }
    pub(crate) mod pend {
        pub(crate) use crate::authority::{Export, Pend};
        pub(crate) use crate::ledger::{PendEntry, locate};
        pub(crate) use crate::target::Target;
        pub(crate) use kernel_api::model::modifiable::Modifiable;
        pub(crate) enum Need {
            Read,
            Write,
        }
        impl Need {
            pub(crate) fn allowed_by(self, access: Modifiable) -> bool {
                matches!(self, Self::Read) || access == Modifiable::Yes
            }
        }
        pub(crate) fn live(task: &super::unit::task::Task) -> bool {
            !task.dead.load(std::sync::atomic::Ordering::Acquire)
        }
    }
    pub(crate) mod nest {
        pub(crate) use crate::support::{Nest, Span};
        pub(crate) mod authority {
            pub(crate) fn withdraw_source_branch(
                _: &std::sync::Arc<crate::support::Sharing>,
                _: kernel_api::model::identity::PendId,
                _: bool,
            ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
                panic!("source withdrawal is outside the registration fixture")
            }
        }
    }
    pub(crate) mod view {
        pub(crate) use crate::support::{CloneView, View};
    }
    pub(crate) mod flat {
        pub(crate) mod sharing {
            pub(crate) use crate::support::{Sharing, SharingOperation};
            pub(crate) fn seal(_: &Sharing) {
                panic!("outside registration fixture")
            }
        }
        pub(crate) mod authority {
            pub(crate) fn retire_block(
                _: &std::sync::Arc<crate::support::Sharing>,
                _: &std::sync::Arc<crate::authority::Pend>,
            ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
                panic!("Flat retirement is outside the registration fixture")
            }
        }
    }
    pub(crate) mod sign {
        pub(crate) use crate::support::WakeKey;
        pub(crate) fn signal(_: WakeKey) {}
        pub(crate) fn wake(_: WakeKey, _: &std::sync::Weak<crate::support::Anchor>) {}
        pub(crate) mod endpoint {
            pub(crate) use crate::support::Endpoint;
            use kernel_api::model::identity::PendId;
            pub(crate) fn seal(_: &Endpoint) {
                panic!("outside registration fixture")
            }
            pub(crate) fn release_authority(_: &Endpoint, _: PendId) {
                panic!("outside registration fixture")
            }
            pub(crate) fn authority_changed(_: &Endpoint) {
                panic!("outside registration fixture")
            }
            pub(crate) fn trigger_key(_: &Endpoint) -> super::WakeKey {
                panic!("outside registration fixture")
            }
        }
    }
    pub(crate) mod room {
        pub(crate) mod interrupt {
            pub(crate) fn withdraw(_: kernel_api::model::identity::PendId) {}
            pub(crate) fn dispatch(
                _: &std::sync::Arc<crate::support::Task>,
                _: &std::sync::Arc<crate::ledger::PendEntry>,
            ) -> Result<(), kernel_api::abi::call::pend::PendFail> {
                Ok(())
            }
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
        pub(crate) use std::sync::OnceLock;
        pub(crate) type SpinLockGuard<'a, T> = std::sync::MutexGuard<'a, T>;
    }
}

fn entry() -> Arc<PendEntry> {
    Arc::new(
        PendEntry::try_new(
            target::Target::None {
                owner: kernel_api::model::identity::TaskId::new(0),
            },
            kernel_api::model::identity::Mark::NONE,
            kernel_api::model::modifiable::Modifiable::No,
            None,
        )
        .unwrap(),
    )
}

#[test]
fn unpublished_pend_rolls_back_and_keeps_one_record_on_publish() {
    let table = ledger::Table::new().unwrap();
    let _transaction = table.lock();
    let entry = entry();
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
        let entry = entry();
        assert!(allocation::fail_after(fail, || table.prepare(entry.clone())).is_err());
        table.prepare(entry).unwrap().publish();
        assert_eq!(table.entries.lock().len(), 1);
    }
    let table = ledger::Table::new().unwrap();
    let records: Vec<_> = (1..=8).map(|_| entry()).collect();
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
