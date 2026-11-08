//! Pend authority and observation.

use kernel_api::abi::call::pend::operations as op;
use kernel_api::{
    abi::call::pend::PendFail, address::VirtAddr, model::identity::PendId, model::identity::TaskId,
};

typed_calls! {
    fallible fn accord(request: VirtAddr) -> Result<PendId, erra::Error<PendFail>> =
        op::Accord { request };
    fallible fn narrow(request: VirtAddr) -> Result<(), erra::Error<PendFail>> =
        op::Narrow { request };
    fallible fn revoke(dst: TaskId, token: PendId) -> Result<(), erra::Error<PendFail>> =
        op::Revoke { dst, token };
    fallible fn return_(token: PendId) -> Result<(), erra::Error<PendFail>> =
        op::Return { token };
    fallible fn inspect(token: PendId, buf: VirtAddr) -> Result<(), erra::Error<PendFail>> =
        op::Inspect { token, buf };
    fallible fn collect(after: PendId, buf: VirtAddr, capacity: usize) -> Result<usize, erra::Error<PendFail>> =
        op::Collect { after, buf, capacity };
}
