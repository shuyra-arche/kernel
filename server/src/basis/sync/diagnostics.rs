//! Bind the shared acquisition mechanism to basis's resource ordering rules.
use super::order::Class;
use crate::metal::generic::sync;
#[cfg(debug_assertions)]
use crate::metal::generic::sync::Admission;
pub(crate) struct Diagnostics;
impl sync::Diagnostics for Diagnostics {
    type Class = Class;
    type Pair = Pair;
    #[cfg(debug_assertions)]
    fn before(addr: usize, class: Class, caller: usize, admission: Admission<Pair>) {
        match admission {
            Admission::Ordinary => super::deadlock::before(addr, class, caller),
            Admission::Pair {
                predecessor,
                policy,
            } => super::deadlock::before_pair(addr, predecessor, class, policy, caller),
            Admission::Borrow => super::deadlock::before_source_borrow(addr, caller),
        }
    }
    #[cfg(debug_assertions)]
    fn acquired(addr: usize, class: Class, caller: usize, admission: Admission<Pair>) {
        use super::deadlock::Kind;
        let kind = match admission {
            Admission::Ordinary => Kind::Ordinary,
            Admission::Pair { predecessor, .. } => Kind::Pair { predecessor },
            Admission::Borrow => Kind::SourceBorrow,
        };
        super::deadlock::acquired(addr, class, caller, kind);
    }
    #[cfg(debug_assertions)]
    fn released(addr: usize) {
        super::deadlock::released(addr);
    }
}
#[cfg(debug_assertions)]
pub(crate) use super::deadlock::Pair;
#[cfg(not(debug_assertions))]
#[derive(Clone, Copy)]
#[allow(
    dead_code,
    reason = "pair metadata is consumed only by debug diagnostics"
)]
pub(crate) enum Pair {
    Flat,
    Ledger { first: usize, second: usize },
}
