//! Resident Kernel Team Tasks, grouped by independently scheduled responsibility.
//! Boot constructs and starts each Task explicitly; this module is not a dispatcher.
//! Keep entry, startup, wake sources and bounded loop together in each child.
//! Authoritative resource state and retirement evidence remain with basis owners.
pub(crate) mod observe;

pub(crate) mod lifecycle;
fn wait() {
    crate::metal::switcher::internal::wait();
}
fn yield_now() {
    crate::metal::switcher::internal::sleep(0);
}
fn exit() -> ! {
    crate::metal::switcher::internal::exit()
}
