//! Continue an accepted maintenance drain through its retained credential.
//! A failed request remains failed in the journal; recovery is an explicit new
//! operation on that exact Block, never another Block allocation.
use crate::support::request;
use kernel_api::abi::memory::*;
use kernel_client::{
    call::room,
    memory::{Error, Response},
};

fn invoke(request: Request) -> Result<Response, Error> {
    match Call::from_request(request) {
        Call::Flat(c) => kernel_client::memory::flat::invoke(c),
        Call::Nest(c) => kernel_client::memory::nest::invoke(c),
    }
}

pub fn call(mut action: Action) -> Result<Response, Error> {
    let deadline = room::clock() + 5_000_000_000;
    loop {
        let request = request(action);
        let response = invoke(request)?;
        if let (Action::Retract { memo, .. }, Response::Reply(reply)) = (action, &response) {
            if let Outcome::Failed {
                failure: Failure::Busy,
                retained: Some(Value::Memo { generation }),
            } = reply.outcome()
            {
                let Response::Reply(replay) = invoke(request)? else {
                    panic!("replay")
                };
                assert_eq!(replay, *reply);
                assert!(room::clock() < deadline, "retained Memo did not retire");
                action = Action::Retract {
                    memo,
                    expected: generation,
                };
                room::yield_();
                continue;
            }
        }
        if let (Action::Block { start, count, .. }, Response::Reply(reply)) = (action, &response) {
            if let Outcome::Failed {
                failure: Failure::Busy,
                retained: Some(Value::Maintenance { block, .. }),
            } = reply.outcome()
            {
                // Verify lost-delivery recovery before advancing the retained
                // owner. Replaying must not allocate another Block or retry it.
                let Response::Reply(replay) = invoke(request)? else {
                    panic!("replay")
                };
                assert_eq!(replay, *reply);
                assert!(room::clock() < deadline, "retained Block did not drain");
                action = Action::Block {
                    start,
                    count,
                    recover: Some(block),
                };
                room::yield_();
                continue;
            }
        }
        return Ok(response);
    }
}

pub fn block(action: Action) -> Result<Response, Error> {
    let deadline = room::clock() + 5_000_000_000;
    loop {
        let response = call(action)?;
        if matches!(action, Action::Block { .. })
            && matches!(&response, Response::Reply(r) if r.outcome() == Outcome::Rejected(Failure::Busy))
        {
            assert!(room::clock() < deadline, "Block admission stayed busy");
            room::yield_();
            continue;
        }
        return Ok(response);
    }
}
