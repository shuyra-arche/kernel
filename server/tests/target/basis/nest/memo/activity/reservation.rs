//! Fresh optimistic namespace versions for the concurrently retiring fixture.
//! Only rejected requests are rebuilt. Accepted identities are never replayed
//! as a new operation, and every retry rereads the actual Nest revision.
use crate::{
    pause,
    support::{call, inspect, pend_info},
};
use kernel_api::{
    abi::memory::*,
    address::VirtAddr,
    model::{
        identity::PendId,
        nest::{MemoPosition, CreateSpanRequest},
        pend::Target,
    },
};
use kernel_client::{call::room, memory::Response};
pub fn reserve(token: PendId, reservation: CreateSpanRequest) -> (MemoPosition, VirtAddr) {
    let Target::Nest(nest) = pend_info(token).target else {
        panic!("Nest")
    };
    let end = room::clock() + 8_000_000_000;
    loop {
        let Snapshot::Nest(state) = inspect(Query::Nest { token, nest }) else {
            panic!("Nest snapshot")
        };
        let Response::Reply(reply) = call(Action::CreateSpan {
            token,
            nest,
            expected: state.version,
            reservation,
        }) else {
            panic!("Reserve reply")
        };
        match reply.outcome() {
            Outcome::Committed(Value::Span {
                token,
                nest,
                reservation,
                address,
                ..
            }) => {
                return (
                    MemoPosition::new(token, nest, reservation, 0).unwrap(),
                    address,
                );
            }
            Outcome::Rejected(Failure::Busy | Failure::Stale) => {
                assert!(room::clock() < end);
                pause();
            }
            other => panic!("Reserve {other:?}"),
        }
    }
}
