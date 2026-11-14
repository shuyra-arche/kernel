//! Production lock-order predicates with real atomics/guards and host IRQs.
#[cfg(debug_assertions)]
use super::{depend, reentrant::RelLock};
use super::{level::Level, metal, pair, spin::SpinLock, trap, verdict};
#[cfg(debug_assertions)]
use std::panic::{AssertUnwindSafe, catch_unwind};

#[cfg(debug_assertions)]
#[test]
fn forbidden_try_checks_before_reserving_a_turn_and_restores_the_original_irqs() {
    metal::hart::set(0);
    for (parent, child) in [
        (Level::Flat, Level::Nest),
        (Level::Nest, Level::Unit),
        (Level::Scheduler, Level::Unit),
        (Level::Flat, Level::Flat),
    ] {
        let higher = SpinLock::new_level(parent, ());
        let lower = SpinLock::new_level(child, ());
        {
            let _held = higher.lock();
            assert!(catch_unwind(AssertUnwindSafe(|| lower.try_lock())).is_err());
            assert!(!trap::enabled());
            assert_eq!(depend::count(), 1);
        }
        assert!(trap::enabled());
        // A precheck panic must not reserve a ticket or acquire a phantom lock.
        assert!(lower.try_lock().is_some());
        assert_eq!(depend::count(), 0);
    }
}

#[cfg(debug_assertions)]
#[test]
fn nonrecursive_try_rejects_same_instance_before_ticket_or_irq_changes_escape() {
    metal::hart::set(0);
    let lock = SpinLock::new(0);
    {
        let _held = lock.lock();
        assert!(catch_unwind(AssertUnwindSafe(|| lock.try_lock())).is_err());
        assert!(!trap::enabled());
        assert_eq!(depend::count(), 1);
    }
    assert!(lock.try_lock().is_some());
    assert!(trap::enabled());
}

#[cfg(debug_assertions)]
#[test]
fn recursive_try_cannot_claim_a_new_commit_turn_before_diagnosing_inverse_order() {
    metal::hart::set(0);
    let state = SpinLock::new_level(Level::Nest, ());
    let commit = RelLock::new_level(Level::Unit, ());
    {
        let _state = state.lock();
        assert!(catch_unwind(AssertUnwindSafe(|| commit.try_lock())).is_err());
        assert_eq!(depend::count(), 1);
    }
    assert!(trap::enabled());
    // Previously RelLock's check occurred after try_enter, so a diagnostic
    // panic could strand the next ticket even though no Guard existed.
    assert!(commit.try_lock().is_some());
}

#[test]
fn production_predicates_require_exact_flat_pair_predecessor_and_only_one_pair() {
    let held = [(10, Some(Level::Unit)), (20, Some(Level::Flat))];
    assert!(verdict::peer(held, 30, 20, Some(Level::Flat)).is_ok());
    for (address, lower, level) in [
        (20, 20, Level::Flat),
        (15, 20, Level::Flat),
        (30, 19, Level::Flat),
        (30, 20, Level::Nest),
    ] {
        assert!(verdict::peer(held, address, lower, Some(level)).is_err());
    }
    assert!(
        verdict::peer(
            [(20, Some(Level::Flat)), (30, Some(Level::Flat))],
            40,
            30,
            Some(Level::Flat)
        )
        .is_err()
    );
    assert!(
        verdict::peer(
            [(20, Some(Level::Flat)), (30, Some(Level::UnitState))],
            40,
            20,
            Some(Level::Flat)
        )
        .is_err()
    );
    assert!(verdict::strict(held, 30, Some(Level::Flat)).is_err());
}

#[test]
fn ordered_pair_uses_real_distinct_instances_and_releases_before_restoring_irqs() {
    metal::hart::set(0);
    let a = SpinLock::new_level(Level::Flat, 11);
    let b = SpinLock::new_level(Level::Flat, 17);
    pair::with_pair(&b, &a, |b, a| {
        assert!(!trap::enabled());
        *a += 1;
        *b += 1;
    });
    assert!(trap::enabled());
    assert_eq!(*a.lock(), 12);
    assert_eq!(*b.lock(), 18);
    // Same-level guards above must not overlap merely for observation.
}

#[test]
fn legacy_total_order_uses_the_production_categories_without_claiming_a_dag() {
    let levels = [
        Level::Unit,
        Level::Scheduler,
        Level::Address,
        Level::Nest,
        Level::Reservation,
        Level::Flat,
        Level::TeamTasks,
        Level::L3,
        Level::Registry,
        Level::Asid,
        Level::Frame,
        Level::Tally,
        Level::Spare,
        Level::UnitState,
    ];
    for (i, parent) in levels.into_iter().enumerate() {
        for (j, child) in levels.into_iter().enumerate() {
            assert_eq!(
                verdict::strict([(1, Some(parent))], 2, Some(child)).is_ok(),
                j > i
            );
        }
    }
}
