//! Cell creation, transfer and publication.

use kernel_api::abi::call::cell::operations as op;
use kernel_api::{
    abi::call::cell::CellFail,
    address::VirtAddr,
    model::{cell::CreatedCell, identity::PendId},
};

typed_calls! {
    fallible fn create(license: PendId, request: VirtAddr) -> Result<CreatedCell, erra::Error<CellFail>> = op::Create { license, request };
    fallible fn delete(object: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Delete { object };
    fallible fn push(request: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Push { request };
    fallible fn pull(request: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Pull { request };
    fallible fn publish(position: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Publish { position };
    fallible fn retract(position: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Retract { position };
    fallible fn protect(position: VirtAddr, access: u64) -> Result<(), erra::Error<CellFail>> = op::Protect { position, access };
    fallible fn inspect(position: VirtAddr, out: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Inspect { position, out };
}
