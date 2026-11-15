#![allow(dead_code, unused_imports)]
pub(crate) struct Boundary {
    private: (),
}
pub(crate) mod sync;
pub(crate) fn boundary<R>(body: impl for<'a> FnOnce(sync::Root<'a>) -> R) -> R {
    assert_eq!(
        sync::deadlock::count(),
        0,
        "no new execution boundary under held ancestors"
    );
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(100);
    crate::metal::hart::set(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let mut boundary = Boundary { private: () };
    body(unsafe { sync::enter(&mut boundary) })
}

#[path = "../../../../../src/metal/generic/sync/mod.rs"]
pub(crate) mod generic;
// SAFETY: the host fixture only lends its private token inside boundary().
unsafe impl generic::Boundary for Boundary {}

pub(crate) struct Environment;
// SAFETY: host IRQ fixture uses one pinned host thread per simulated Hart.
unsafe impl generic::Environment for Environment {
    type Guard = sync::trap::TrapGuard;
    unsafe fn save() -> Self::Guard {
        unsafe { Self::Guard::save() }
    }
    fn owner() -> usize {
        crate::metal::hart::hart_id().get()
    }
    fn wait() {
        Self::poll();
    }
    fn poll() {
        crate::metal::interrupt::window::poll();
    }
    #[cfg(debug_assertions)]
    fn caller() -> usize {
        0
    }
}
