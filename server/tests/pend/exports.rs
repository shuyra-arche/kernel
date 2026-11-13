//! Real logical export guards; no MMU completion is simulated here.
use crate::authority::{Export, Pend};
use kernel_api::{
    abi::call::pend::PendFail,
    model::{
        modifiable::Modifiable::{No, Yes},
        pend::{Distributable, FlatRights, Operations, PendRange, PendScope, ShareRights},
    },
};
use std::sync::{Arc, Barrier, Mutex};

fn scope(offset: usize, count: usize) -> PendScope {
    PendScope::Pages(PendRange::Pages { offset, count })
}

#[test]
fn distribution_is_independent_of_write_and_validates_the_complete_selection() {
    for operations in [
        Operations::Flat(FlatRights::ASSOCIATE),
        Operations::Share(ShareRights::ASSOCIATE),
    ] {
        for access in [No, Yes] {
            for distribution in [Distributable::No, Distributable::Yes] {
                let root =
                    Pend::try_new_scope(access, scope(4, 8), operations, distribution).unwrap();
                for wrong in [scope(3, 2), scope(11, 2), scope(4, 0), scope(usize::MAX, 2)] {
                    assert_eq!(
                        Export::new(&root, operations, wrong).err(),
                        Some(PendFail::Denied)
                    );
                }
                let export = Export::new(&root, operations, scope(6, 2));
                if distribution == Distributable::No {
                    assert_eq!(export.err(), Some(PendFail::Denied));
                    assert!(!root.external_pending());
                } else {
                    let export = export.unwrap();
                    assert!(export.permits(No));
                    assert_eq!(export.permits(Yes), access == Yes);
                    assert!(root.external_pending());
                    drop(export);
                    assert!(!root.external_pending());
                }
            }
        }
    }
}

#[test]
fn export_retains_logical_obligation_until_last_copy_but_revocation_is_immediate() {
    let ops = Operations::Share(ShareRights::ASSOCIATE);
    let root = Pend::try_new_scope(Yes, scope(0, 8), ops, Distributable::Yes).unwrap();
    let original = root.limits();
    let export = Arc::new(Export::new(&root, ops, scope(2, 2)).unwrap());
    let copy = export.clone();
    assert_eq!(root.can_narrow(original), Err(PendFail::Busy));
    assert_eq!(root.narrow(original), Err(PendFail::Busy));
    assert_eq!(root.limits(), original);
    root.invalidate();
    assert!(!export.permits(No));
    assert_eq!(
        Export::new(&root, ops, scope(2, 2)).err(),
        Some(PendFail::Dead)
    );
    assert_eq!(root.settle(), Err(PendFail::Busy));
    drop(export);
    assert_eq!(root.settle(), Err(PendFail::Busy));
    drop(copy);
    root.settle().unwrap();
}

#[test]
fn ancestor_revocation_invalidates_exports_without_affecting_a_sibling() {
    let ops = Operations::Share(ShareRights::ASSOCIATE);
    let make = || Pend::try_new_scope(No, scope(0, 8), ops, Distributable::Yes).unwrap();
    let root = make();
    let branch = make();
    branch.inherit(&root).unwrap();
    let leaf = make();
    leaf.inherit(&branch).unwrap();
    let sibling = make();
    sibling.inherit(&root).unwrap();
    let export = Export::new(&leaf, ops, scope(0, 1)).unwrap();
    let independent = Export::new(&sibling, ops, scope(0, 1)).unwrap();
    branch.invalidate();
    assert!(!export.permits(No));
    assert!(independent.permits(No));
}

#[test]
fn serialized_export_and_narrow_cannot_both_commit() {
    for _ in 0..32 {
        let ops = Operations::Share(ShareRights::ASSOCIATE);
        let root = Pend::try_new_scope(Yes, scope(0, 8), ops, Distributable::Yes).unwrap();
        let commit = Mutex::new(());
        let start = Barrier::new(2);
        std::thread::scope(|threads| {
            let exporter = threads.spawn(|| {
                start.wait();
                let _commit = commit.lock().unwrap();
                Export::new(&root, ops, scope(0, 1))
            });
            start.wait();
            let narrowed = {
                let _commit = commit.lock().unwrap();
                root.narrow(kernel_api::model::pend::Limits {
                    distributable: Distributable::No,
                    ..root.limits()
                })
            };
            match exporter.join().unwrap() {
                Ok(export) => {
                    assert_eq!(narrowed, Err(PendFail::Busy));
                    assert!(export.permits(Yes));
                }
                Err(error) => {
                    assert_eq!(error, PendFail::Denied);
                    assert_eq!(narrowed, Ok(()));
                    assert!(!root.external_pending());
                }
            }
        });
    }
}
