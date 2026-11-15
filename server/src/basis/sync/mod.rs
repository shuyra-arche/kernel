//! Resource lock categories, typed ancestor protocol and the unique transaction
//! boundary. Hardware execution protection comes from metal's selected environment.
#[cfg(debug_assertions)]
pub(crate) mod deadlock;
mod diagnostics;
mod global;
mod legacy;
pub(crate) mod lock;
pub(crate) mod order;
mod root;
use crate::metal::generic::sync::{Admission, reentrant, spin};
use crate::metal::sync::Environment;
use diagnostics::Diagnostics;
type SpinLock<T> = spin::SpinLock<T, Environment, Diagnostics>;
type SpinLockGuard<'a, T> = spin::SpinLockGuard<'a, T, Environment, Diagnostics>;
type RelLock<T> = reentrant::RelLock<T, Environment, Diagnostics>;
type RelLockGuard<'a, T> = reentrant::RelLockGuard<'a, T, Environment, Diagnostics>;
pub(crate) use global::global_commit;
#[cfg(debug_assertions)]
pub(crate) use global::global_held_by_current;
pub(crate) use legacy::LegacyLock;
pub(crate) use root::Root;
pub(crate) type LegacyGuard<'a, T> = SpinLockGuard<'a, T>;
pub(crate) type LegacyCommitGuard<'a, T> = RelLockGuard<'a, T>;
#[cfg(debug_assertions)]
pub(crate) use deadlock::DeadlockInitError;
#[cfg(debug_assertions)]
pub(crate) fn init_deadlock(hart_count: usize) -> Result<(), DeadlockInitError> {
    deadlock::init(hart_count)
}

/// # Safety
/// The caller establishes an actual Task-call boundary; no guards survive it.
pub(crate) unsafe fn enter(boundary: &mut impl crate::metal::generic::sync::Boundary) -> Root<'_> {
    unsafe { Root::enter(boundary) }
}
