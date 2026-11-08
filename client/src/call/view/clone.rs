//! Copied View payloads and queue observation.

use kernel_api::abi::call::clone_view::operations as op;
use kernel_api::{
    abi::call::clone_view::CloneFail, address::VirtAddr, model::clone_view::CloneLimits,
    model::clone_view::PullOutcome, model::identity::Mark, model::identity::PendId,
    model::identity::TaskId,
};

typed_calls! {
    fallible fn create(license: PendId, mark: Mark, limits: CloneLimits) -> Result<PendId, erra::Error<CloneFail>> =
        op::Create { license, mark, limits };
    fallible fn delete(token: PendId) -> Result<(), erra::Error<CloneFail>> =
        op::Delete { token };
    fallible fn push(token: PendId, msg: VirtAddr, len: usize) -> Result<(), erra::Error<CloneFail>> =
        op::Push { token, msg, len };
    fallible fn pull(token: PendId, max_len: usize) -> Result<PullOutcome, erra::Error<CloneFail>> =
        op::Pull { token, max_len };
    fallible fn peek(token: PendId) -> Result<(usize, TaskId, usize), erra::Error<CloneFail>> =
        op::Peek { token };
}
