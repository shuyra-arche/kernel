#![feature(allocator_ext)]
extern crate alloc;
mod ancestry;
#[allow(dead_code)]
#[path = "../../src/basis/pend/authority/mod.rs"]
mod authority;
mod metal {
    pub(crate) mod sync {
        pub(crate) use std::sync::OnceLock;
        pub(crate) type SpinLockGuard<'a, T> = std::sync::MutexGuard<'a, T>;
        pub(crate) struct SpinLock<T>(std::sync::Mutex<T>);
        impl<T> SpinLock<T> {
            pub(crate) fn new(value: T) -> Self {
                Self(std::sync::Mutex::new(value))
            }
            pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, T> {
                self.0.lock().unwrap()
            }
        }
    }
}
mod ranges;
mod scopes;
#[path = "../../src/basis/pend/authority/state.rs"]
mod state;
#[path = "../../src/basis/pend/window.rs"]
mod window;
use kernel_api::model::modifiable::Modifiable::No;
use kernel_api::model::modifiable::Modifiable::Yes;
use state::Event::*;
use state::Failure;
use state::Phase;
use state::State;

#[test]
fn mutable_parent_is_suspended_until_every_reader_returns() {
    let root = State::new(Yes);
    let one = root.transition(Borrow(No)).unwrap();
    let two = one.transition(Borrow(No)).unwrap();
    assert!(!one.usable() && !two.usable());
    assert_eq!(two.transition(Borrow(Yes)), Err(Failure::Busy));
    assert_eq!(two.transition(Narrow(No)), Err(Failure::Busy));
    let one = two.transition(Return(No)).unwrap();
    assert!(!one.usable());
    assert_eq!(one.transition(Return(No)), Ok(root));
    assert_eq!(root.transition(Return(No)), Err(Failure::Denied));
}
#[test]
fn shared_parent_remains_readable_but_cannot_produce_a_writer() {
    let root = State::new(No);
    assert_eq!(root.transition(Borrow(Yes)), Err(Failure::Denied));
    assert_eq!(root.transition(Narrow(Yes)), Err(Failure::Denied));
    assert!(root.transition(Borrow(No)).unwrap().usable());
}
#[test]
fn mutable_reborrow_excludes_every_sibling() {
    let root = State::new(Yes);
    let borrowed = root.transition(Borrow(Yes)).unwrap();
    assert!(!borrowed.usable());
    for access in [No, Yes] {
        assert_eq!(borrowed.transition(Borrow(access)), Err(Failure::Busy));
    }
    assert_eq!(borrowed.transition(Return(No)), Err(Failure::Denied));
    assert_eq!(borrowed.transition(Return(Yes)), Ok(root));
}
#[test]
fn retirement_never_resumes_access_and_waits_for_children() {
    let borrowed = State::new(Yes).transition(Borrow(No)).unwrap();
    let retiring = borrowed.transition(Retire).unwrap();
    assert_eq!(retiring.phase, Phase::Retiring);
    assert!(!retiring.usable());
    assert_eq!(retiring.transition(Settle), Err(Failure::Busy));
    assert_eq!(retiring.transition(Borrow(No)), Err(Failure::Dead));
    let returned = retiring.transition(Return(No)).unwrap();
    assert!(!returned.usable());
    let dead = returned.transition(Settle).unwrap();
    assert_eq!(dead.phase, Phase::Dead);
    assert_eq!(dead.transition(Retire), Ok(dead));
    assert!(!dead.usable());
}

#[test]
fn cancellation_and_settlement_require_different_proofs() {
    let active = State::new(Yes);
    assert_eq!(active.transition(Settle), Err(Failure::Denied));
    assert_eq!(active.transition(Cancel).unwrap().phase, Phase::Dead);
    let borrowed = active.transition(Borrow(No)).unwrap();
    assert_eq!(borrowed.transition(Cancel), Err(Failure::Busy));
    let retiring = active.transition(Retire).unwrap();
    assert_eq!(retiring.transition(Cancel), Err(Failure::Denied));
    assert_eq!(retiring.transition(Settle).unwrap().phase, Phase::Dead);
}

#[test]
fn live_permission_restricts_operations_without_changing_borrow_rules() {
    use kernel_api::model::pend::{Distributable, Operations, PendRange, PendScope, ShareRights};
    let all = Operations::Share(ShareRights::all());
    let open = Operations::Share(ShareRights::OPEN);
    let scope = PendScope::Pages(PendRange::Pages {
        offset: 0,
        count: 8,
    });
    let root = State::authorized(Yes, scope, all, Distributable::Yes);
    let limited = root
        .transition(Limit {
            access: Yes,
            scope,
            operations: open,
            distributable: Distributable::No,
        })
        .unwrap();
    assert!(limited.permits(open));
    assert!(!limited.permits(Operations::Share(ShareRights::ASSOCIATE)));
    assert_eq!(
        limited.transition(Limit {
            access: Yes,
            scope,
            operations: all,
            distributable: Distributable::No
        }),
        Err(Failure::Denied)
    );
    assert_eq!(
        limited.transition(Limit {
            access: Yes,
            scope,
            operations: open,
            distributable: Distributable::Yes
        }),
        Err(Failure::Denied)
    );
    let lent = limited.transition(Borrow(No)).unwrap();
    assert!(!lent.permits(open));
    assert_eq!(
        lent.transition(Limit {
            access: No,
            scope,
            operations: open,
            distributable: Distributable::No
        }),
        Err(Failure::Busy)
    );
    let back = lent.transition(Return(No)).unwrap();
    assert!(back.permits(open));
    assert!(!back.transition(Retire).unwrap().permits(open));
}
