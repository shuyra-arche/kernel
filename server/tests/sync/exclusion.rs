use super::{depend, metal, spin::SpinLock, trap};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

#[test]
fn ordinary_lock_waiters_keep_their_turn_and_observe_prior_writes() {
    metal::hart::set(0);
    let lock = Arc::new(SpinLock::new_level(depend::Level::Unit, 0));
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
    let lock = Arc::new(SpinLock::new(7));
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
