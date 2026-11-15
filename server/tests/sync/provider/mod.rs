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
    body(unsafe { sync::Root::enter(&mut boundary) })
}
