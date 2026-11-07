use super::outcome::answer_pair;
use crate::core::work::room::scheduler::roster::muster;
use crate::core::work::{
    mail::Mail,
    pend::{self, Need},
    unit::task::TaskIdent,
};
use crate::machine::{device, switcher::context::TrapContext};
use alloc::sync::Arc;
use kernel_api::abi::call::device::{DeviceCall, DeviceFail};

fn failure(error: crate::core::work::device::pci::Error) -> DeviceFail {
    use crate::core::work::device::pci::Error;
    match error {
        Error::Absent | Error::Denied => DeviceFail::Denied,
        Error::Unsupported => DeviceFail::Unsupported,
        Error::Invalid => DeviceFail::Invalid,
        Error::Busy => DeviceFail::Busy,
        Error::Hardware => DeviceFail::Hardware,
    }
}
pub(super) fn dispatch(frame: &mut TrapContext, call: DeviceCall, ident: &Arc<TaskIdent>) {
    answer_pair(frame, execute(call, ident));
}
fn execute(call: DeviceCall, ident: &Arc<TaskIdent>) -> Result<(usize, usize), DeviceFail> {
    let me = muster(ident.id)
        .and_then(|w| w.upgrade())
        .ok_or(DeviceFail::Denied)?;
    // Serialize identity/authority against exit and delegation changes. All MMIO
    // effects stay in the device lock; nothing accepts a caller-supplied address.
    let _pend = me.pend.lock();
    let _commit = crate::core::work::commit();
    if me.dead() {
        return Err(DeviceFail::Denied);
    }
    if matches!(
        call,
        DeviceCall::Scan { .. } | DeviceCall::Claim { .. } | DeviceCall::Observe { .. }
    ) && !pend::allows_locked(
        &me,
        &Mail::Nole(super::resources::get().device.clone()),
        Need::Fetch,
    ) {
        return Err(DeviceFail::Denied);
    }
    let owner = &ident.team.space.dma;
    match call {
        DeviceCall::Fault { device: id } => device::fault(&me, id).map_err(failure),
        DeviceCall::Observe { device: id } => Ok((device::observe(id).map_err(failure)?, 0)),
        DeviceCall::Publish {
            device: id,
            reference,
            iova,
            modifiable,
        } => {
            let hold = me
                .pend
                .references
                .get(reference.get())
                .ok_or(DeviceFail::Denied)?;
            let borrow =
                pend::ExternalBorrow::prepare(&hold, ident.id, modifiable).map_err(|error| {
                    match error {
                        kernel_api::abi::call::pie::PieFail::OoM => DeviceFail::OoM,
                        kernel_api::abi::call::pie::PieFail::Busy => DeviceFail::Busy,
                        _ => DeviceFail::Denied,
                    }
                })?;
            device::publish(id, owner, borrow, iova, modifiable).map_err(failure)?;
            Ok((0, 0))
        }
        DeviceCall::Status { device: id } => Ok((device::status(id, owner).map_err(failure)?, 0)),
        DeviceCall::Retire { device: id } => {
            device::retire(id, owner).map_err(failure)?;
            Ok((0, 0))
        }
        DeviceCall::Scan { after } => Ok(device::scan(after).unwrap_or((0, 0))),
        DeviceCall::Claim { device: id, team } => {
            let target = if team.get() == 0 {
                ident.team.clone()
            } else {
                me.clan(team).ok_or(DeviceFail::Denied)?
            };
            device::claim(id, &target.space.dma).map_err(failure)?;
            Ok((0, 0))
        }
        DeviceCall::Inspect { device: id, offset } => {
            Ok((device::inspect(id, owner, offset).map_err(failure)?, 0))
        }
        DeviceCall::Read {
            device: id,
            offset,
            width,
        } => {
            let value = device::read(id, owner, offset, width).map_err(failure)?;
            Ok((value as u32 as usize, (value >> 32) as usize))
        }
        DeviceCall::Write {
            device: id,
            offset,
            width,
            value,
        } => {
            device::write(id, owner, offset, width, value).map_err(failure)?;
            Ok((0, 0))
        }
    }
}
