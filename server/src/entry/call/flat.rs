use crate::{
    basis::{flat::authority, room::scheduler::slot::current, unit::task::TaskIdent},
    metal::switcher::context::TrapContext,
};
use alloc::sync::Arc;
use kernel_api::abi::call::{
    flat::{FlatCall, FlatFail},
    pend::PendFail,
};
pub(super) fn dispatch(frame: &mut TrapContext, call: FlatCall, ident: &Arc<TaskIdent>) {
    let result = (|| {
        let task = current().running_task().ok_or(PendFail::Dead)?;
        match call {
            FlatCall::Create {
                license,
                size,
                align,
                level,
            } => authority::create(&task, license, size, align, level).map(|p| p.get()),
            FlatCall::Inspect { token, out } => {
                let info = authority::inspect(&task, token)?;
                if !ident.team.nest.copy_out(&info.to_bytes(), out) {
                    return Err(PendFail::Denied);
                }
                Ok(0)
            }
            FlatCall::Slice {
                token,
                offset,
                count,
            } => authority::slice(&task, token, offset, count),
            FlatCall::Delete { token } => authority::delete(&task, token).map(|()| 0),
        }
    })();
    super::outcome::answer(
        frame,
        result.map_err(|error| match error {
            PendFail::Denied | PendFail::Invalid => FlatFail::Denied,
            PendFail::Dead => FlatFail::Dead,
            PendFail::Busy => FlatFail::Busy,
            PendFail::OutOfMemory => FlatFail::OutOfMemory,
            PendFail::NotAligned => FlatFail::NotAligned,
            PendFail::HandedOver => FlatFail::HandedOver,
        }),
    );
}
