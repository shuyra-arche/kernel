use super::provider;
use provider::sync::{
    deadlock,
    lock::{
        Lock,
        reentry::{Accept, Reject},
    },
    order, trap,
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

#[test]
fn typed_ledger_commit_flat_borrows_actual_ancestors_and_restores_irqs() {
    let serial = Lock::<order::LedgerSerial, Reject, _>::new(());
    let state = Lock::<order::Flat, Reject, _>::new(3);
    provider::boundary(|mut root| {
        let mut ledger = root.lock(&serial);
        assert!(!trap::enabled());
        {
            let mut commit = ledger.lock(provider::sync::global_commit());
            {
                let mut flat = commit.lock(&state);
                *flat += 4;
            }
            assert!(!trap::enabled());
            {
                let nested = commit.reenter();
                assert_eq!(*nested, ());
            }
            assert!(!trap::enabled());
        }
        assert!(!trap::enabled());
    });
    assert!(trap::enabled());
    provider::boundary(|mut root| assert_eq!(*root.lock(&state), 7));
    assert_eq!(deadlock::count(), 0);
}

#[test]
fn accept_reentry_is_the_same_instance_with_shared_access() {
    let lock = Lock::<order::Nest, Accept, _>::new(std::cell::Cell::new(1));
    provider::boundary(|mut root| {
        let mut outer = root.lock(&lock);
        let original = &*outer as *const _;
        {
            let inner = outer.reenter();
            assert_eq!(&*inner as *const _, original);
            inner.set(2);
        }
        assert_eq!(outer.get(), 2);
        #[cfg(debug_assertions)]
        assert_eq!(deadlock::count(), 1);
    });
    assert!(trap::enabled());
}

#[test]
fn flat_pair_preserves_argument_identity_in_both_orders_and_masks_until_both_release() {
    let a = Lock::<order::Flat, Reject, _>::new(11);
    let b = Lock::<order::Flat, Reject, _>::new(17);
    provider::boundary(|mut root| {
        {
            let mut pair = root.pair(&b, &a);
            let (b, a) = pair.values();
            *a += 1;
            *b += 2;
            assert!(!trap::enabled());
        }
        assert!(trap::enabled());
        {
            let mut pair = root.try_pair(&a, &b).unwrap();
            let (a, b) = pair.values();
            assert_eq!((*a, *b), (12, 19));
        }
    });
    assert!(trap::enabled());
}

#[test]
fn failed_second_pair_try_releases_first_without_abandoning_a_turn() {
    let locks = Arc::new([
        Lock::<order::Flat, Reject, _>::new(1),
        Lock::<order::Flat, Reject, _>::new(2),
    ]);
    let other = locks.clone();
    let (held, ready) = mpsc::channel();
    let (release, done) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        provider::boundary(|mut root| {
            let _guard = root.lock(&other[1]);
            held.send(()).unwrap();
            done.recv_timeout(Duration::from_secs(5)).unwrap();
        })
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    provider::boundary(|mut root| {
        assert!(root.try_pair(&locks[0], &locks[1]).is_none());
        assert!(trap::enabled());
        assert_eq!(deadlock::count(), 0);
        assert!(root.try_lock(&locks[0]).is_some());
    });
    release.send(()).unwrap();
    worker.join().unwrap();
    provider::boundary(|mut root| assert!(root.try_pair(&locks[1], &locks[0]).is_some()));
}

#[test]
fn aliased_flat_pair_rejects_before_admission_in_both_profiles() {
    let a = Lock::<order::Flat, Reject, _>::new(0);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        provider::boundary(|mut root| {
            let _pair = root.pair(&a, &a);
        })
    }));
    assert!(result.is_err());
    assert!(trap::enabled());
    assert_eq!(deadlock::count(), 0);
    provider::boundary(|mut root| assert!(root.try_lock(&a).is_some()));
}

#[cfg(debug_assertions)]
#[test]
fn inverse_try_panics_before_ticket_and_preserves_existing_irq_state() {
    use provider::sync::legacy::LegacyLock;
    for inherited_mask in [false, true] {
        let irq = if inherited_mask {
            Some(unsafe { trap::TrapGuard::save() })
        } else {
            None
        };
        let flat = LegacyLock::<order::Flat, _>::new(());
        let nest = LegacyLock::<order::Nest, _>::new(());
        {
            let _flat = flat.lock();
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nest.try_lock())).is_err()
            );
            assert_eq!(deadlock::count(), 1);
            assert!(!trap::enabled());
        }
        assert_eq!(trap::enabled(), !inherited_mask);
        assert!(nest.try_lock().is_some());
        assert_eq!(deadlock::count(), 0);
        drop(irq);
        assert!(trap::enabled());
    }
}

#[cfg(debug_assertions)]
#[test]
fn execution_boundary_rejects_a_root_under_a_live_legacy_ancestor() {
    let flat = provider::sync::legacy::LegacyLock::<order::Flat, _>::new(());
    let guard = flat.lock();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| provider::boundary(|_| ())))
            .is_err()
    );
    assert_eq!(deadlock::count(), 1);
    drop(guard);
    assert!(trap::enabled());
}

#[test]
fn clone_queue_growth_keeps_allocator_leaves_below_both_ancestors() {
    use provider::sync::legacy::LegacyLock;
    let serial = LegacyLock::<order::LedgerSerial, _>::new(());
    let queue = LegacyLock::<order::CloneQueue, _>::new(());
    let pump = LegacyLock::<order::BlockPump, _>::new(());
    let pool = LegacyLock::<order::BlockPool, _>::new(());
    let tally = LegacyLock::<order::Tally, _>::new(());
    let frame = LegacyLock::<order::Frame, _>::new(());
    let _serial = serial.lock();
    let _queue = queue.lock();
    {
        let _pump = pump.lock();
    }
    {
        let _pool = pool.lock();
        {
            let _tally = tally.lock();
        }
        {
            let _frame = frame.lock();
        }
    }
}
