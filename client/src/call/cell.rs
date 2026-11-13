//! ABI v10 compatibility only. The next provider uses `memory::{flat,nest}`.

use kernel_api::abi::call::cell::operations as op;
use kernel_api::{
    abi::call::cell::CellFail,
    address::VirtAddr,
    model::{
        cell::{Allocation, SlotId},
        identity::PendId,
    },
};

typed_calls! {
    fallible fn split(slot: SlotId) -> Result<SlotId, erra::Error<CellFail>> = op::Split { slot };
    fallible fn merge(slot: SlotId) -> Result<SlotId, erra::Error<CellFail>> = op::Merge { slot };
    fallible fn create(license: PendId, allocation: Allocation) -> Result<SlotId, erra::Error<CellFail>> = op::Create { license, allocation };
    fallible fn delete(slot: SlotId) -> Result<(), erra::Error<CellFail>> = op::Delete { slot };
    fallible fn push(request: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Push { request };
    fallible fn pull(request: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Pull { request };
    fallible fn insert(request: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Insert { request };
    fallible fn remove(position: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Remove { position };
    fallible fn publish(position: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Publish { position };
    fallible fn retract(position: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Retract { position };
    fallible fn protect(position: VirtAddr, access: u64) -> Result<(), erra::Error<CellFail>> = op::Protect { position, access };
    fallible fn inspect(position: VirtAddr, out: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Inspect { position, out };
}
