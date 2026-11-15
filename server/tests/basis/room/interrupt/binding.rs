use super::{
    mechanism::service::{BindingError, Service},
    synchronization::Environment,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Init;
// SAFETY: only OnceLock publication/poll is used. Execution-guard operations
// never return and cannot establish a false Hart identity.
unsafe impl Environment for Init {
    type Guard = ();
    unsafe fn save() {
        panic!("not an execution environment")
    }
    fn owner() -> usize {
        panic!("not an execution environment")
    }
    fn wait() {
        std::thread::yield_now();
    }
    fn poll() {
        std::thread::yield_now();
    }
    #[cfg(debug_assertions)]
    fn caller() -> usize {
        0
    }
}
static SEEN: AtomicUsize = AtomicUsize::new(0);
fn receive(value: usize) {
    SEEN.store(value, Ordering::Relaxed);
}
#[test]
fn unset_service_cannot_activate_and_concurrent_binding_has_one_owner() {
    let binding = Arc::new(Service::<Init>::new());
    assert!(matches!(binding.require(), Err(BindingError::Missing)));
    let results: Vec<_> = (0..16)
        .map(|_| {
            let binding = binding.clone();
            std::thread::spawn(move || binding.bind(receive))
        })
        .collect();
    let results: Vec<_> = results.into_iter().map(|r| r.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(BindingError::Duplicate))
            .count(),
        15
    );
    binding.require().unwrap()(73);
    assert_eq!(SEEN.load(Ordering::Relaxed), 73);
    assert_eq!(binding.bind(receive), Err(BindingError::Duplicate));
}
