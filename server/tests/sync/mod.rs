//! Real RelLock atomics/guard ownership with a host Hart/IRQ boundary fixture.
//! These tests prove admission and recursion, not RISC-V interrupt timing.
mod support;
pub(crate) use support::metal;
#[path = "../../src/metal/riscv/sync/reentrant/mod.rs"]
mod reentrant;
use metal::sync::{depend, trap};
use reentrant::RelLock;
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[test]
fn failed_try_returns_while_foreign_owner_is_still_held_and_restores_irqs() {
    metal::hart::set(0);
    let lock = Arc::new(RelLock::new_level(depend::Level::Unit, 17));
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
    let lock = RelLock::new_level(depend::Level::Unit, 29);
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
    let lock = Arc::new(RelLock::new_level(depend::Level::Unit, 41));
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
    let lock = Arc::new(RelLock::new_level(depend::Level::Unit, 41));
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
    let lock = Arc::new(RelLock::new_level(
        depend::Level::Unit,
        std::cell::Cell::new(0),
    ));
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
