//! Production membership and topology checks. Host fabric facts and completion
//! are synthetic; these tests establish software decisions, not physical coherence.
use crate::{
    cell::{Block, Owner, Page, RetainError},
    proof::*,
    support::Pool,
};
use std::{num::NonZeroU64, sync::Arc};
use topology::{Fabric, Ram, Route, Topology};
fn n(n: u64) -> NonZeroU64 {
    NonZeroU64::new(n).unwrap()
}
fn cpu() -> Endpoint {
    Endpoint::Cpu {
        nest: n(1),
        leases: n(1),
    }
}
fn device(requester: u32) -> Endpoint {
    Endpoint::External {
        controller: 0x1000,
        requester,
        binding: n(requester as u64 + 2),
    }
}
fn page() -> Arc<Page> {
    Arc::new(Page::from_internal(Owner::Kernel, Pool::new(0).page().unwrap()).unwrap())
}
fn topology(page: &Page, sets: &[&[u32]]) -> Topology {
    // SAFETY: modeled coherent sets only, never installed on actual hardware.
    unsafe {
        Topology::new(
            sets.iter()
                .map(|routes| Fabric {
                    ram: vec![Ram {
                        base: page.physical_address() as u64,
                        bytes: page.size() as u64,
                    }],
                    routes: routes
                        .iter()
                        .map(|&requester| Route {
                            controller: 0x1000,
                            requester,
                        })
                        .collect(),
                })
                .collect(),
        )
        .unwrap()
    }
}
#[test]
fn qualified_writers_settle_individually_and_prepared_or_retiring_members_still_count() {
    let page = page();
    let topology = topology(&page, &[&[0, 1]]);
    let change = page.begin().unwrap();
    let write = Access::new(6).unwrap();
    let mut cpu = change.qualified(&topology, cpu(), write, None).unwrap();
    let mut first = change.qualified(&topology, device(0), write, None).unwrap();
    let mut second = change.qualified(&topology, device(1), write, None).unwrap();
    assert_eq!(page.obligations(), 3);
    cpu.publish();
    first.publish();
    second.publish();
    let first = first.retire();
    assert_eq!(page.obligations(), 3);
    unsafe { first.settle() };
    assert_eq!(page.obligations(), 2);
    assert!(!page.allows_return());
    unsafe {
        second.retire().settle();
        cpu.retire().settle();
    }
    drop(change);
    assert!(page.allows_return());
}
#[test]
fn pairwise_fabrics_cannot_authorize_a_larger_group() {
    let page = page();
    let topology = topology(&page, &[&[0], &[1]]);
    let change = page.begin().unwrap();
    let read = Access::new(2).unwrap();
    let cpu = change.qualified(&topology, cpu(), read, None).unwrap();
    let first = change.qualified(&topology, device(0), read, None).unwrap();
    assert!(matches!(
        change.qualified(&topology, device(1), read, None),
        Err(RetainError::Borrowed)
    ));
    assert_eq!(page.obligations(), 2);
    drop(first);
    drop(change.qualified(&topology, device(1), read, None).unwrap());
    drop(cpu);
}
#[test]
fn unknown_legacy_aliases_and_unknown_routes_refuse_without_losing_receipts() {
    let page = page();
    let topology = topology(&page, &[&[0]]);
    let legacy = page.prepare_hardware(false).unwrap();
    let change = page.begin().unwrap();
    let read = Access::new(2).unwrap();
    assert!(change.qualified(&topology, device(0), read, None).is_err());
    assert_eq!(page.obligations(), 1);
    drop(legacy);
    let qualified = change.qualified(&topology, cpu(), read, None).unwrap();
    assert!(change.external(false).is_err());
    assert!(change.hardware(false).is_err());
    assert!(change.qualified(&topology, device(1), read, None).is_err());
    assert_eq!(page.obligations(), 1);
    drop(qualified);
}
#[test]
fn group_bound_and_overlapping_blocks_do_not_evict_existing_members() {
    let page = page();
    let topology = topology(&page, &[&[0]]);
    let change = page.begin().unwrap();
    let block = Block::new(change.clone()).unwrap();
    let permit = block.permit().unwrap();
    let mut entries = Vec::new();
    for _ in 0..MAX_MEMBERS {
        entries.push(
            change
                .qualified(&topology, device(0), Access::new(6).unwrap(), Some(&permit))
                .unwrap(),
        );
    }
    assert!(matches!(
        change.qualified(&topology, cpu(), Access::new(2).unwrap(), Some(&permit)),
        Err(RetainError::Exhausted)
    ));
    assert_eq!(page.obligations(), MAX_MEMBERS as u64);
    let other = Block::new(change.clone()).unwrap();
    drop(entries.pop());
    assert!(matches!(
        change.qualified(&topology, cpu(), Access::new(2).unwrap(), Some(&permit)),
        Err(RetainError::Busy)
    ));
    drop(entries);
    drop(other);
    drop(permit);
    drop(block);
    drop(change);
    assert!(page.allows_return());
}
#[test]
fn unsupported_memory_execution_and_lost_completion_remain_closed() {
    let page = page();
    let topology = topology(&page, &[&[0]]);
    let unrelated = self::page();
    let other = unrelated.begin().unwrap();
    assert!(
        other
            .qualified(&topology, cpu(), Access::new(2).unwrap(), None)
            .is_err()
    );
    let change = page.begin().unwrap();
    assert!(
        change
            .qualified(&topology, cpu(), Access::new(8).unwrap(), None)
            .is_err()
    );
    let mut receipt = change
        .qualified(&topology, device(0), Access::new(6).unwrap(), None)
        .unwrap();
    receipt.publish();
    drop(receipt.retire());
    assert!(page.quarantined());
    assert_eq!(page.obligations(), 1);
    assert!(
        change
            .qualified(&topology, cpu(), Access::new(2).unwrap(), None)
            .is_err()
    );
}
#[test]
fn topology_rejects_overflow_overlapping_intervals_and_duplicate_routes() {
    for ram in [
        vec![Ram {
            base: u64::MAX - 4095,
            bytes: 4096,
        }],
        vec![
            Ram {
                base: 0x1000,
                bytes: 8192,
            },
            Ram {
                base: 0x2000,
                bytes: 4096,
            },
        ],
    ] {
        assert!(
            unsafe {
                Topology::new(vec![Fabric {
                    ram,
                    routes: vec![],
                }])
            }
            .is_err()
        );
    }
    let route = Route {
        controller: 0x1000,
        requester: 0,
    };
    assert!(
        unsafe {
            Topology::new(vec![Fabric {
                ram: vec![Ram {
                    base: 0x8000,
                    bytes: 4096,
                }],
                routes: vec![route, route],
            }])
        }
        .is_err()
    );
}

#[test]
fn racing_incompatible_devices_cannot_both_enter_from_the_same_snapshot() {
    let page = page();
    let topology = topology(&page, &[&[0], &[1]]);
    let change = page.begin().unwrap();
    let start = std::sync::Barrier::new(3);
    let results = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            start.wait();
            change.qualified(&topology, device(0), Access::new(6).unwrap(), None)
        });
        let b = scope.spawn(|| {
            start.wait();
            change.qualified(&topology, device(1), Access::new(6).unwrap(), None)
        });
        start.wait();
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(page.obligations(), 1);
    drop(results);
    drop(change);
    assert!(page.allows_return());
}
#[test]
fn failed_membership_preparation_never_registers_a_phantom_obligation() {
    let page = page();
    let topology = topology(&page, &[&[0]]);
    let change = page.begin().unwrap();
    let first = change
        .qualified(&topology, cpu(), Access::new(2).unwrap(), None)
        .unwrap();
    assert!(matches!(
        crate::allocation::fail_after(0, || {
            change.qualified(&topology, device(0), Access::new(6).unwrap(), None)
        }),
        Err(RetainError::Exhausted)
    ));
    assert_eq!(page.obligations(), 1);
    drop(first);
    drop(
        change
            .qualified(&topology, device(0), Access::new(6).unwrap(), None)
            .unwrap(),
    );
    assert_eq!(page.obligations(), 0);
}
