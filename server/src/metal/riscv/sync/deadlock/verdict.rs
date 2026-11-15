//! Production predicates: bounded caller-supplied entries, no locks, allocation
//! or panic. The static Before implementations and this check share order.
use super::super::order::{Class, before};
#[derive(Clone, Copy)]
pub(crate) enum Pair {
    Flat,
    Ledger { first: usize, second: usize },
}
pub(crate) fn strict(
    held: impl IntoIterator<Item = (usize, Class)>,
    addr: usize,
    class: Class,
) -> Result<(), &'static str> {
    for (at, parent) in held {
        if at == addr {
            return Err("recursive Reject acquisition");
        }
        if !before(parent, class) {
            return Err("class is not reachable from every held ancestor");
        }
    }
    Ok(())
}
pub(crate) fn peer(
    held: impl IntoIterator<Item = (usize, Class)>,
    addr: usize,
    predecessor: usize,
    class: Class,
    pair: Pair,
) -> Result<(), &'static str> {
    match (class, pair) {
        (Class::Flat, Pair::Flat) if predecessor < addr => {}
        (Class::LedgerSerial, Pair::Ledger { first, second }) if first < second => {}
        _ => return Err("invalid pair class or stable instance order"),
    }
    let (mut same, mut found) = (0, false);
    for (at, parent) in held {
        if at == addr {
            return Err("pair aliases its real predecessor");
        }
        if parent == class {
            same += 1;
            found = at == predecessor;
        } else if !before(parent, class) {
            return Err("pair under an incomparable or later ancestor");
        }
    }
    if same != 1 || !found {
        return Err("pair requires exactly its one real predecessor");
    }
    Ok(())
}
/// One audited try-only inversion for Source::borrow, never ordinary admission.
/// Its successors must still satisfy strict against both Commit and Ledger.
pub(crate) fn source_borrow(
    held: impl IntoIterator<Item = (usize, Class)>,
    addr: usize,
) -> Result<(), &'static str> {
    let (mut commit, mut ledgers) = (0, 0);
    for (at, class) in held {
        if at == addr {
            return Err("source ledger already held");
        }
        match class {
            Class::Commit => commit += 1,
            Class::LedgerSerial => ledgers += 1,
            other if before(other, Class::Commit) => {}
            _ => return Err("source borrow try under a resource storage guard"),
        }
    }
    if commit != 1 || ledgers > 1 {
        return Err("source borrow requires the real Commit and at most one publisher ledger");
    }
    Ok(())
}
