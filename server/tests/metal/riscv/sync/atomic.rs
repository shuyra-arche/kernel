use super::{order::Class, reentrant::RelLock, spin::SpinLock};
use crate::metal;
use crate::metal::sync::{deadlock as depend, trap};
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[test]
fn failed_try_returns_while_foreign_owner_is_still_held_and_restores_irqs() {
    metal::hart::set(0);
    let lock = Arc::new(RelLock::new(Class::Commit, 17));
    let held = lock.lock();
    assert!(!trap::enabled());
    let (sent, received) = mpsc::channel();
    let other = lock.clone();
    let worker = std::thread::spawn(move || {
        metal::hart::set(1);
        assert!(trap::enabled());
        assert!(other.try_lock().is_none());
        assert!(trap::enabled());
        assert_eq!(depend::count(), 0);
        let irq = unsafe { trap::TrapGuard::save() };
        assert!(other.try_lock().is_none());
        assert!(!trap::enabled());
        drop(irq);
        sent.send(()).unwrap();
    });
    // Keep the foreign owner held until the rejection is observed. A blocking
    // implementation cannot satisfy this handshake (timeout is only a watchdog).
    let result = received.recv_timeout(Duration::from_secs(5));
    assert_eq!(*held, 17);
    #[cfg(debug_assertions)]
    assert!(lock.held_by_current());
    drop(held);
    worker.join().unwrap();
    result.unwrap();
    assert!(trap::enabled());
    assert_eq!(depend::count(), 0);
    let acquired = lock.try_lock().unwrap();
    assert_eq!(*acquired, 17);
}

#[test]
fn recursive_try_preserves_outer_ownership_and_irq_mask_until_last_guard() {
    metal::hart::set(2);
    let lock = RelLock::new(Class::Commit, 29);
    let outer = lock.try_lock().unwrap();
    #[cfg(debug_assertions)]
    assert_eq!(depend::count(), 1);
    let inner = lock.try_lock().unwrap();
    #[cfg(debug_assertions)]
    assert_eq!(depend::count(), 1);
    assert_eq!(*inner, 29);
    drop(inner);
    assert!(!trap::enabled());
    #[cfg(debug_assertions)]
    assert!(lock.held_by_current());
    drop(outer);
    #[cfg(debug_assertions)]
    assert!(!lock.held_by_current());
    assert_eq!(depend::count(), 0);
    assert!(trap::enabled());
}

#[test]
fn contended_wait_visits_reception_before_foreign_owner_releases() {
    metal::hart::set(0);
    let lock = Arc::new(RelLock::new(Class::Commit, 41));
    let owner = lock.lock();
    let (sent, received) = mpsc::channel();
    let other = lock.clone();
    let worker = std::thread::spawn(move || {
        metal::hart::set(1);
        metal::interrupt::window::observe(sent);
        let acquired = other.lock();
        assert_eq!(*acquired, 41);
        assert!(!trap::enabled());
        drop(acquired);
        assert!(trap::enabled());
    });
    let reception = received.recv_timeout(Duration::from_secs(5));
    drop(owner);
    worker.join().unwrap();
    reception.unwrap();
}

#[test]
fn queued_waiter_cannot_be_bypassed_even_between_owners() {
    metal::hart::set(0);
    let lock = Arc::new(RelLock::new(Class::Commit, 41));
    let owner = lock.lock();
    let (notice, observed) = mpsc::channel();
    let (resume, paused) = mpsc::channel();
    let other = lock.clone();
    let waiter = std::thread::spawn(move || {
        metal::hart::set(1);
        metal::interrupt::window::pause(notice, paused);
        assert_eq!(*other.lock(), 41);
        assert!(trap::enabled());
    });
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    // Recursion must remain possible with a foreign reservation outstanding.
    drop(lock.lock());
    drop(lock.try_lock().unwrap());
    drop(owner);
    // The waiter is paused before acquiring: owner is free, queue is not.
    let barged = lock.try_lock().is_some();
    assert!(trap::enabled());
    resume.send(()).unwrap();
    waiter.join().unwrap();
    assert!(!barged, "a new try bypassed the retained waiter");
    assert!(lock.try_lock().is_some(), "failed try abandoned a ticket");
}

#[test]
fn queued_harts_enter_in_arrival_order_and_publish_protected_writes() {
    metal::hart::set(0);
    let lock = Arc::new(RelLock::new(Class::Commit, std::cell::Cell::new(0)));
    let owner = lock.lock();
    let mut workers = Vec::new();
    for id in 1..=4 {
        let (notice, observed) = mpsc::channel();
        let other = lock.clone();
        workers.push(std::thread::spawn(move || {
            metal::hart::set(id);
            metal::interrupt::window::observe(notice);
            let held = other.lock();
            assert_eq!(held.get(), id - 1);
            held.set(id);
        }));
        // Arrival is acknowledged by the actual contended reception path,
        // not a sleep or an assumption about host thread scheduling.
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    drop(owner);
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(lock.lock().get(), 4);
}

#[test]
fn ordinary_lock_waiters_keep_their_turn_and_observe_prior_writes() {
    metal::hart::set(0);
    let lock = Arc::new(SpinLock::new(Class::LedgerSerial, 0));
    let owner = lock.lock();
    let mut workers = Vec::new();
    for id in 1..=4 {
        let (notice, observed) = mpsc::channel();
        let other = lock.clone();
        workers.push(std::thread::spawn(move || {
            metal::hart::set(id);
            metal::interrupt::window::observe(notice);
            let mut held = other.lock();
            assert_eq!(*held, id - 1);
            *held = id;
            drop(held);
            assert!(trap::enabled());
            assert_eq!(depend::count(), 0);
        }));
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    drop(owner);
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(*lock.lock(), 4);
}

#[test]
fn ordinary_try_cannot_steal_a_reserved_turn_or_abandon_one_on_failure() {
    metal::hart::set(0);
    let lock = Arc::new(SpinLock::new(Class::LedgerSerial, 7));
    let owner = lock.lock();
    let (notice, observed) = mpsc::channel();
    let (resume, paused) = mpsc::channel();
    let other = lock.clone();
    let waiter = std::thread::spawn(move || {
        metal::hart::set(1);
        assert!(other.try_lock().is_none());
        assert!(trap::enabled());
        metal::interrupt::window::pause(notice, paused);
        *other.lock() = 9;
    });
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(owner);
    let barged = lock.try_lock().is_some();
    assert!(trap::enabled());
    resume.send(()).unwrap();
    waiter.join().unwrap();
    assert!(!barged);
    assert_eq!(*lock.try_lock().expect("failed try left no ticket"), 9);
}

#[cfg(debug_assertions)]
#[test]
fn reentrant_first_try_checks_inverse_before_reserving_a_turn() {
    let nest = SpinLock::new(Class::Nest, ());
    let commit = RelLock::new(Class::Commit, ());
    {
        let _nest = nest.lock();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| commit.try_lock())).is_err()
        );
        assert!(!trap::enabled());
        assert_eq!(depend::count(), 1);
    }
    assert!(trap::enabled());
    assert!(commit.try_lock().is_some());
    assert_eq!(depend::count(), 0);
}

#[test]
fn actual_ledger_peer_releases_second_first_and_same_owner_uses_one_turn() {
    use super::{
        lock::{LedgerSerialPairGuard, Lock, reentry::Reject},
        order,
    };
    let a = Lock::<order::LedgerSerial, Reject, _>::new(());
    let b = Lock::<order::LedgerSerial, Reject, _>::new(());
    {
        // Trusted mechanism test couples these actual instances to stable IDs.
        let _pair = unsafe { LedgerSerialPairGuard::legacy(&b, 9, &a, 7) }.unwrap();
        assert!(!trap::enabled());
        #[cfg(debug_assertions)]
        assert_eq!(depend::count(), 2);
    }
    assert!(trap::enabled());
    assert_eq!(depend::count(), 0);
    {
        let _same = unsafe { LedgerSerialPairGuard::legacy(&a, 7, &a, 7) }.unwrap();
        #[cfg(debug_assertions)]
        assert_eq!(depend::count(), 1);
    }
    assert!(trap::enabled());
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            LedgerSerialPairGuard::legacy(&a, 7, &b, 7)
        }))
        .is_err()
    );
    assert!(trap::enabled());
    assert_eq!(depend::count(), 0);
}

#[cfg(debug_assertions)]
#[test]
fn source_reverse_try_uses_real_commit_and_failed_foreign_admission_records_nothing() {
    let ledger = Arc::new(SpinLock::new(Class::LedgerSerial, ()));
    let source = ledger.clone();
    let (notice, ready) = mpsc::channel();
    let (release, done) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        metal::hart::set(23);
        let _held = source.lock();
        notice.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    crate::provider::boundary(|_| {
        {
            let _commit = unsafe { super::global_commit().legacy_lock() };
            assert!(ledger.try_source_borrow().is_none());
            assert_eq!(depend::count(), 1);
            assert!(!trap::enabled());
        }
        assert!(trap::enabled());
        assert_eq!(depend::count(), 0);
    });
    release.send(()).unwrap();
    worker.join().unwrap();
    crate::provider::boundary(|_| {
        let _commit = unsafe { super::global_commit().legacy_lock() };
        {
            let _source = ledger.try_source_borrow().unwrap();
            assert_eq!(depend::count(), 2);
        }
        assert_eq!(depend::count(), 1);
    });
    assert!(trap::enabled());
    assert_eq!(depend::count(), 0);
}
