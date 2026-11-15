//! Debug-only acquisition-order diagnostics. This is not a cross-Hart wait-cycle
//! detector. IRQ-masked access and a fixed per-Hart pool avoid allocator locks.
use super::{OnceLock, order::Class};
use crate::metal::hart;
use alloc::boxed::Box;
use core::cell::UnsafeCell;
mod error;
mod held;
pub(crate) mod verdict;
pub use error::DeadlockInitError;
pub(crate) use held::Kind;
use held::{Held, HeldCell, HeldSet};
pub(crate) use verdict::Pair;
static POOL: OnceLock<&'static [HeldCell]> = OnceLock::new();
pub(crate) fn init(hart_count: usize) -> Result<(), DeadlockInitError> {
    if POOL.get().is_some() {
        return Err(DeadlockInitError::AlreadyInit);
    }
    let n = hart_count.clamp(1, crate::metal::layout::MAX_HART_SLOTS);
    let mut cells =
        Box::<[HeldCell]>::try_new_uninit_slice(n).map_err(|_| DeadlockInitError::OutOfMemory)?;
    for cell in &mut cells {
        cell.write(HeldCell(UnsafeCell::new(HeldSet::new())));
    }
    POOL.set(Box::leak(unsafe { cells.assume_init() }))
        .map_err(|_| DeadlockInitError::AlreadyInit)
}
fn held() -> Option<&'static mut HeldSet> {
    let pool = POOL.get()?;
    let h = hart::hart_id();
    assert!(
        h.get() < pool.len(),
        "[deadlock] Hart out of preallocated pool"
    );
    Some(unsafe { &mut *pool[h.get()].0.get() })
}
fn report(what: &'static str, addr: usize, caller: usize) -> ! {
    let snapshot = held().map(|h| h.snapshot());
    panic!("[deadlock] {what}: lock={addr:#x} caller={caller:#x} held={snapshot:?}");
}
pub(crate) fn before(addr: usize, class: Class, caller: usize) {
    let Some(set) = held() else { return }; // explicitly audited bootstrap only
    if let Err(why) = verdict::strict(set.entries().iter().map(|h| (h.addr, h.class)), addr, class)
    {
        report(why, addr, caller)
    }
}
pub(crate) fn before_pair(
    addr: usize,
    predecessor: usize,
    class: Class,
    pair: Pair,
    caller: usize,
) {
    let Some(set) = held() else { return };
    if let Err(why) = verdict::peer(
        set.entries().iter().map(|h| (h.addr, h.class)),
        addr,
        predecessor,
        class,
        pair,
    ) {
        report(why, addr, caller)
    }
}
pub(crate) fn before_source_borrow(addr: usize, caller: usize) {
    assert!(
        super::global_held_by_current(),
        "source borrow requires the actual global Commit instance"
    );
    let set = held().expect("runtime source borrow requires initialized tracking");
    if let Err(why) = verdict::source_borrow(set.entries().iter().map(|h| (h.addr, h.class)), addr)
    {
        report(why, addr, caller)
    }
}
pub(crate) fn acquired(addr: usize, class: Class, caller: usize, kind: Kind) {
    let Some(set) = held() else { return };
    if set
        .push(Held {
            addr,
            class,
            caller,
            kind,
        })
        .is_err()
    {
        report("64 held instances exhausted", addr, caller)
    }
}
pub(crate) fn released(addr: usize) {
    let Some(set) = held() else { return };
    if set.remove(addr).is_err() {
        report("unheld release or live pair successor", addr, 0)
    }
}
pub(crate) fn root() {
    let set = held().expect("Task Root requires initialized tracking");
    if !set.is_empty() {
        report("Root with live ancestors", 0, 0)
    }
}
