//! Unit lifecycle and management.

use kernel_api::abi::call::unit::operations as op;
use kernel_api::{
    abi::call::unit::UnitFail,
    address::VirtAddr,
    model::identity::{TaskId, TeamId},
    model::unit::{ControlId, UnitQuery, UnitTarget},
};

typed_calls! {
    fallible fn create(request: VirtAddr) -> Result<UnitTarget, erra::Error<UnitFail>> = op::Create { request };
    fallible fn delete(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> = op::Delete { target };
    fallible fn resume(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> = op::Resume { target };
    fallible fn freeze(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> = op::Freeze { target };
    fallible fn stop(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> = op::Stop { target };
    terminal fn exit(value: u64, reason: u64, note: VirtAddr, len: usize) -> ! = op::Exit { value, reason, note, len };
    fallible fn finish(team: TeamId) -> Result<(), erra::Error<UnitFail>> = op::Finish { team };
    fallible fn inspect(query: UnitQuery, out: VirtAddr) -> Result<bool, erra::Error<UnitFail>> = op::Inspect { query, out };
    fallible fn accord(authority: ControlId, target: TaskId, delegable: bool) -> Result<ControlId, erra::Error<UnitFail>> = op::Accord { authority, target, delegable };
    fallible fn revoke(authority: ControlId, target: TaskId, branch: ControlId) -> Result<(), erra::Error<UnitFail>> = op::Revoke { authority, target, branch };
    fallible fn return_(authority: ControlId) -> Result<(), erra::Error<UnitFail>> = op::Return { authority };
    fallible fn admit(task: TaskId, schedule: VirtAddr) -> Result<(), erra::Error<UnitFail>> = op::Admit { task, schedule };
    fallible fn evict(task: TaskId) -> Result<(), erra::Error<UnitFail>> = op::Evict { task };
    fallible fn profile(task: TaskId, out: VirtAddr) -> Result<(), erra::Error<UnitFail>> = op::Profile { task, out };
}
