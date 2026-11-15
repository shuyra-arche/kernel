#![allow(dead_code, unused_imports)]
#[cfg(debug_assertions)]
#[path = "../../../../../../../src/metal/riscv/sync/deadlock/held.rs"]
pub(crate) mod held;
#[cfg(debug_assertions)]
#[path = "../../../../../../../src/metal/riscv/sync/deadlock/verdict.rs"]
pub(crate) mod verdict;
#[cfg(debug_assertions)]
pub(crate) use held::Kind;
#[cfg(debug_assertions)]
pub(crate) use verdict::Pair;
#[cfg(debug_assertions)]
use {
    super::order::Class,
    held::{Held, HeldSet},
};
#[cfg(debug_assertions)]
std::thread_local! {static HELD:std::cell::RefCell<HeldSet>=const {std::cell::RefCell::new(HeldSet::new())};}
#[cfg(debug_assertions)]
pub(crate) fn before(addr: usize, class: Class, _: usize) {
    HELD.with_borrow(|h| {
        verdict::strict(h.entries().iter().map(|h| (h.addr, h.class)), addr, class).unwrap()
    });
}
#[cfg(debug_assertions)]
pub(crate) fn before_pair(addr: usize, pre: usize, class: Class, pair: Pair, _: usize) {
    HELD.with_borrow(|h| {
        verdict::peer(
            h.entries().iter().map(|h| (h.addr, h.class)),
            addr,
            pre,
            class,
            pair,
        )
        .unwrap()
    });
}
#[cfg(debug_assertions)]
pub(crate) fn before_source_borrow(addr: usize, _: usize) {
    assert!(super::global_held_by_current());
    HELD.with_borrow(|h| {
        verdict::source_borrow(h.entries().iter().map(|h| (h.addr, h.class)), addr).unwrap()
    });
}
#[cfg(debug_assertions)]
pub(crate) fn acquired(addr: usize, class: Class, caller: usize, kind: Kind) {
    HELD.with_borrow_mut(|h| {
        h.push(Held {
            addr,
            class,
            caller,
            kind,
        })
        .unwrap()
    });
}
#[cfg(debug_assertions)]
pub(crate) fn released(addr: usize) {
    HELD.with_borrow_mut(|h| h.remove(addr).unwrap());
}
#[cfg(debug_assertions)]
pub(crate) fn root() {
    HELD.with_borrow(|h| assert!(h.is_empty()));
}
pub(crate) fn count() -> usize {
    #[cfg(debug_assertions)]
    {
        HELD.with_borrow(|h| h.entries().len())
    }
    #[cfg(not(debug_assertions))]
    {
        0
    }
}
#[cfg(all(test, debug_assertions))]
#[path = "../../../diagnostics.rs"]
mod tests;
