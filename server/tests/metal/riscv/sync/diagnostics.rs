use super::{
    held::{CAPACITY, Held, HeldSet, Kind},
    verdict::{self, Pair},
};
use crate::provider::sync::order::{self, Class};
#[test]
fn dag_checks_every_ancestor_and_has_no_numeric_or_self_fallback() {
    for &a in &Class::ALL {
        assert!(!order::before(a, a));
        for &b in &Class::ALL {
            if order::before(a, b) {
                assert!(!order::before(b, a));
                for &c in &Class::ALL {
                    if order::before(b, c) {
                        assert!(
                            order::before(a, c),
                            "missing transitive edge {a:?} -> {c:?} via {b:?}"
                        );
                    }
                }
            }
        }
    }
    assert!(verdict::strict([(1, Class::Commit), (2, Class::Nest)], 3, Class::Flat).is_ok());
    assert!(verdict::strict([(1, Class::Flat), (2, Class::Commit)], 3, Class::Nest).is_err());
    assert!(verdict::strict([(1, Class::Frame)], 2, Class::Scheduler).is_err());
    // Console address translation must remain outside ledger publication.
    assert!(
        verdict::strict(
            [
                (1, Class::LedgerSerial),
                (2, Class::Commit),
                (3, Class::LedgerEntries)
            ],
            4,
            Class::Nest
        )
        .is_err()
    );
}
#[test]
fn peers_require_actual_predecessor_distinct_instance_and_at_most_two() {
    let held = [(1, Class::Commit), (20, Class::Flat)];
    assert!(verdict::peer(held, 30, 20, Class::Flat, Pair::Flat).is_ok());
    for (at, pre) in [(20, 20), (10, 20), (30, 19)] {
        assert!(verdict::peer(held, at, pre, Class::Flat, Pair::Flat).is_err());
    }
    assert!(
        verdict::peer(
            [(20, Class::Flat), (30, Class::Flat)],
            40,
            30,
            Class::Flat,
            Pair::Flat
        )
        .is_err()
    );
    assert!(
        verdict::peer(
            [(50, Class::LedgerSerial)],
            20,
            50,
            Class::LedgerSerial,
            Pair::Ledger {
                first: 7,
                second: 8
            }
        )
        .is_ok()
    );
    assert!(
        verdict::peer(
            [(50, Class::LedgerSerial)],
            20,
            50,
            Class::LedgerSerial,
            Pair::Ledger {
                first: 8,
                second: 7
            }
        )
        .is_err()
    );
}
#[test]
fn source_borrow_exception_is_try_only_with_no_resource_ancestor_or_third_ledger() {
    assert!(verdict::source_borrow([(1, Class::Commit)], 3).is_ok());
    assert!(verdict::source_borrow([(1, Class::LedgerSerial), (2, Class::Commit)], 3).is_ok());
    for held in [
        vec![],
        vec![(1, Class::Flat), (2, Class::Commit)],
        vec![
            (1, Class::LedgerSerial),
            (2, Class::LedgerSerial),
            (4, Class::Commit),
        ],
    ] {
        assert!(verdict::source_borrow(held, 3).is_err());
    }
    assert!(
        verdict::strict(
            [(1, Class::Commit), (2, Class::LedgerSerial)],
            3,
            Class::Commit
        )
        .is_err()
    );
}
#[test]
fn bounded_storage_has_no_growth_and_keeps_peer_predecessor_until_successor_releases() {
    let mut h = HeldSet::new();
    for addr in 0..CAPACITY {
        h.push(Held {
            addr,
            class: Class::Flat,
            caller: addr + 100,
            kind: Kind::Ordinary,
        })
        .unwrap();
    }
    assert!(
        h.push(Held {
            addr: 99,
            class: Class::Flat,
            caller: 0,
            kind: Kind::Ordinary
        })
        .is_err()
    );
    assert_eq!(h.entries().len(), CAPACITY);
    assert!(format!("{:?}", h.snapshot()).contains("omitted=48"));
    for addr in (0..CAPACITY).rev() {
        h.remove(addr).unwrap();
    }
    assert!(h.is_empty());
    assert!(h.remove(9).is_err());
    h.push(Held {
        addr: 1,
        class: Class::Flat,
        caller: 0,
        kind: Kind::Ordinary,
    })
    .unwrap();
    h.push(Held {
        addr: 2,
        class: Class::Flat,
        caller: 0,
        kind: Kind::Pair { predecessor: 1 },
    })
    .unwrap();
    assert!(h.remove(1).is_err());
    h.remove(2).unwrap();
    h.remove(1).unwrap();
}
