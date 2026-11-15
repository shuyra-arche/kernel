//! Resource lock categories, typed ancestor protocol and the unique transaction
//! boundary. Hardware execution protection comes from metal's selected environment.
pub(crate) mod deadlock;
#[path = "../../../../../../src/basis/sync/diagnostics.rs"]
mod diagnostics;
#[path = "../../../../../../src/basis/sync/global.rs"]
mod global;
#[path = "../../../../../../src/basis/sync/legacy.rs"]
pub(crate) mod legacy;
#[path = "../../../../../../src/basis/sync/lock/mod.rs"]
pub(crate) mod lock;
#[path = "../../../../../../src/basis/sync/order/mod.rs"]
pub(crate) mod order;
#[path = "../../../../../../src/basis/sync/root.rs"]
mod root;
use super::Environment;
use crate::provider::generic::{Admission, reentrant as rawreentrant, spin as rawspin};
use diagnostics::Diagnostics;
pub(crate) type SpinLock<T> = rawspin::SpinLock<T, Environment, Diagnostics>;
pub(crate) type SpinLockGuard<'a, T> = rawspin::SpinLockGuard<'a, T, Environment, Diagnostics>;
pub(crate) type RelLock<T> = rawreentrant::RelLock<T, Environment, Diagnostics>;
pub(crate) type RelLockGuard<'a, T> = rawreentrant::RelLockGuard<'a, T, Environment, Diagnostics>;
pub(crate) use global::global_commit;
#[cfg(debug_assertions)]
pub(crate) use global::global_held_by_current;
pub(crate) use legacy::LegacyLock;
pub(crate) use root::Root;
pub(crate) type LegacyGuard<'a, T> = SpinLockGuard<'a, T>;
pub(crate) type LegacyCommitGuard<'a, T> = RelLockGuard<'a, T>;

/// # Safety
/// The caller establishes an actual Task-call boundary; no guards survive it.
pub(crate) unsafe fn enter(boundary: &mut impl crate::metal::generic::sync::Boundary) -> Root<'_> {
    unsafe { Root::enter(boundary) }
}

pub(crate) mod trap;
pub(crate) mod spin {
    pub(crate) use super::{SpinLock, SpinLockGuard};
}
pub(crate) mod reentrant {
    pub(crate) use super::{RelLock, RelLockGuard};
}
pub(crate) type OnceLock<T> = crate::provider::generic::once::OnceLock<T, super::Environment>;
#[cfg(test)]
#[path = "../../atomic.rs"]
mod atomic;
