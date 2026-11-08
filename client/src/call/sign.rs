//! Sign endpoints and event routing.

use kernel_api::abi::call::sign::operations as op;
use kernel_api::{
    abi::call::sign::{SignFail, SignReply},
    model::identity::TaskId,
    model::sign::SignSource,
    model::sign::{SignId, SignRights},
    model::wait::Wait,
};

typed_calls! {
    fallible fn create() -> Result<SignId, erra::Error<SignFail>> = op::Create;
    fallible fn delete(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Delete { token };
    fallible fn listen(token: SignId, wait: Wait) -> Result<SignReply, erra::Error<SignFail>> = op::Listen { token, wait };
    fallible fn trigger(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Trigger { token };
    fallible fn consume(token: SignId, sequence: u64) -> Result<(), erra::Error<SignFail>> = op::Consume { token, sequence };
    fallible fn attach(token: SignId, source: SignSource) -> Result<(), erra::Error<SignFail>> = op::Attach { token, source };
    fallible fn detach(token: SignId, source: SignSource) -> Result<(), erra::Error<SignFail>> = op::Detach { token, source };
    fallible fn accord(token: SignId, target: TaskId, rights: SignRights) -> Result<SignId, erra::Error<SignFail>> = op::Accord { token, target, rights };
    fallible fn return_(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Return { token };
    fallible fn revoke(token: SignId, target: TaskId, branch: SignId) -> Result<(), erra::Error<SignFail>> = op::Revoke { token, target, branch };
}
