//! Public ABI acceptance: each worker has its own real Task ledger.
mod lineage;
mod maintenance;
mod scopes;
mod unit;
mod worker;
use super::support::*;
use kernel_api::{
    abi::memory::*,
    address::VirtAddr,
    model::{identity::*, nest::*, pend::*, unit::*},
};
use kernel_client::{call::pend, memory::Response};

pub fn run() {
    unit::run();
    scopes::run();
    lineage::run();
    maintenance::run();
}

fn caller() -> TaskId {
    let UnitTarget::Task(id) = task_info(UnitQuery::Caller).target else {
        panic!("Task")
    };
    id
}
fn derive(token: PendId, limits: Limits) -> PendId {
    accord(token, caller(), limits)
}
fn narrow(token: PendId, limits: Limits) {
    let bytes = NarrowRequest { token, limits }.to_bytes();
    let deadline = kernel_client::call::room::clock() + 3_000_000_000;
    loop {
        match pend::narrow(VirtAddr::new(bytes.as_ptr() as usize)) {
            Ok(()) => break,
            Err(e) => assert_eq!(e.into_source(), kernel_api::abi::call::pend::PendFail::Busy),
        }
        assert!(kernel_client::call::room::clock() < deadline);
        kernel_client::call::room::yield_();
    }
}
fn at(memo: MemoPosition, token: PendId, page: usize) -> MemoPosition {
    MemoPosition::new(token, memo.nest(), memo.reservation(), page).unwrap()
}
fn reservation(memo: MemoPosition) -> ReservationStatus {
    let Snapshot::Reservation(s) = inspect(Query::Reservation {
        token: memo.token(),
        nest: memo.nest(),
        reservation: memo.reservation(),
    }) else {
        panic!("Reservation")
    };
    s
}
fn release(memo: MemoPosition) -> Action {
    Action::Release {
        token: memo.token(),
        nest: memo.nest(),
        reservation: memo.reservation(),
        expected: reservation(memo).version,
    }
}
#[track_caller]
fn rejected(response: Result<Response, kernel_client::memory::Error>) -> Failure {
    let response = match response {
        Ok(r) => r,
        Err(kernel_client::memory::Error::Delivery(f)) => return f,
        Err(e) => panic!("{e:?}"),
    };
    let Response::Reply(r) = response else {
        panic!("unexpected observation")
    };
    let Outcome::Rejected(f) = r.outcome() else {
        panic!("expected rejection: {:?}", r.outcome())
    };
    f
}

fn try_call(action: Action) -> Result<Response, kernel_client::memory::Error> {
    crate::recovery::call(action)
}
