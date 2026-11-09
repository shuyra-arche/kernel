use crate::{
    basis::{pend, unit::task::TaskIdent},
    metal::switcher::context::TrapContext,
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::port::{PortCall, PortFail},
    model::{
        pend::{Operations, PortRights},
        port::PortInfo,
    },
};
pub(super) fn dispatch(frame: &mut TrapContext, call: PortCall, ident: &Arc<TaskIdent>) {
    super::outcome::answer(
        frame,
        (|| {
            let actor = crate::basis::room::conductor::current()
                .running_task()
                .ok_or(PortFail::Denied)?;
            let _serial = actor.pend.lock();
            let _commit = crate::basis::commit();
            let PortCall::Inspect { token, out } = call;
            let entry = pend::locate(&actor, token)
                .filter(|e| e.permits(Operations::Port(PortRights::INSPECT)))
                .ok_or(PortFail::Denied)?;
            let port = entry.target().port().ok_or(PortFail::Denied)?;
            let info = PortInfo {
                id: port.id(),
                controller: port.controller,
                requester: port.requester,
                address_bits: 38,
                page_size: 4096,
                window_size: 1 << 21,
            };
            if !ident.team.nest.copy_out(&info.to_bytes(), out) {
                return Err(PortFail::Denied);
            }
            Ok(0)
        })(),
    );
}
