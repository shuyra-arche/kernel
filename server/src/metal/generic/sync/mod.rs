//! FIFO exclusion and RAII guards, statically composed with an execution environment
//! and the caller's lock diagnostics. No resource ordering or ISA is selected here.
pub(crate) mod once;
pub(crate) mod reentrant;
pub(crate) mod spin;
mod turn;

/// # Safety
/// save() must pin execution to its current Hart and mask local interruption
/// until its non-Send guard is dropped. owner() is stable and unique while saved;
/// wait()/poll() may service only work safe under arbitrary held locks.
pub(crate) unsafe trait Environment {
    type Guard;
    unsafe fn save() -> Self::Guard;
    fn owner() -> usize;
    fn wait();
    fn poll();
    #[cfg(debug_assertions)]
    fn caller() -> usize;
}

#[derive(Clone, Copy)]
#[cfg_attr(
    not(debug_assertions),
    allow(
        dead_code,
        reason = "peer metadata is consumed only by debug diagnostics"
    )
)]
pub(crate) enum Admission<P> {
    Ordinary,
    Pair { predecessor: usize, policy: P },
    Borrow,
}

/// Resource owners supply their own categories and admission rules.
pub(crate) trait Diagnostics {
    type Class: Copy + Send + Sync;
    type Pair: Copy;
    #[cfg(debug_assertions)]
    fn before(addr: usize, class: Self::Class, caller: usize, admission: Admission<Self::Pair>);
    #[cfg(debug_assertions)]
    fn acquired(addr: usize, class: Self::Class, caller: usize, admission: Admission<Self::Pair>);
    #[cfg(debug_assertions)]
    fn released(addr: usize);
}

/// # Safety
/// Only a trusted Task-call entry may construct or lend an implementing token.
/// Its borrow ends before switching stacks or returning to the Task.
pub(crate) unsafe trait Boundary {}

#[cfg(debug_assertions)]
pub(crate) mod deadlock;
