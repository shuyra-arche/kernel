//! Sign endpoints and event routing.

use kernel_api::abi::call::sign::operations as op;
use kernel_api::{
    abi::call::sign::{SignFail, SignReply},
    model::identity::PendId,
    model::sign::SignSource,
    model::wait::Wait,
};

typed_calls! {
    fallible fn create(license: PendId) -> Result<PendId, erra::Error<SignFail>> = op::Create { license };
    fallible fn delete(token: PendId) -> Result<(), erra::Error<SignFail>> = op::Delete { token };
    fallible fn listen(token: PendId, wait: Wait) -> Result<SignReply, erra::Error<SignFail>> = op::Listen { token, wait };
    fallible fn trigger(token: PendId) -> Result<(), erra::Error<SignFail>> = op::Trigger { token };
    fallible fn consume(token: PendId, sequence: u64) -> Result<(), erra::Error<SignFail>> = op::Consume { token, sequence };
    fallible fn attach(token: PendId, source: SignSource, generation: u64) -> Result<(), erra::Error<SignFail>> = op::Attach { token, source, generation };
    fallible fn detach(token: PendId, source: SignSource) -> Result<(), erra::Error<SignFail>> = op::Detach { token, source };
    fallible fn claim(token: PendId) -> Result<u64, erra::Error<SignFail>> = op::Claim { token };
    fallible fn yield_binding(token: PendId, generation: u64) -> Result<(), erra::Error<SignFail>> = op::Yield { token, generation };
    fallible fn rearm(token: PendId, generation: u64, sequence: u64) -> Result<(), erra::Error<SignFail>> = op::Rearm { token, generation, sequence };

}
