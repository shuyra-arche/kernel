//! The one exclusion mechanism for basis's global commit boundary. Only this
//! module instantiates its sealed class; basis owns what commits under it.
use super::{
    lock::{Lock, reentry::Accept},
    order::Commit,
};
static COMMIT: Lock<Commit, Accept, ()> = Lock::global();
pub(crate) fn global_commit() -> &'static Lock<Commit, Accept, ()> {
    &COMMIT
}

#[cfg(debug_assertions)]
pub(crate) fn global_held_by_current() -> bool {
    COMMIT.raw.held_by_current()
}
