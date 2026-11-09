use crate::{
    basis::{
        nest::{Nest, SpaceKind, authority::binding},
        pend,
        unit::task::TaskIdent,
    },
    metal::switcher::context::TrapContext,
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::nest::{NestCall, NestFail},
    model::{
        identity::{Mark, PendId},
        modifiable::Modifiable,
        nest::NestKind,
        pend::CreationRights,
    },
};
pub(super) fn dispatch(frame: &mut TrapContext, call: NestCall, ident: &Arc<TaskIdent>) {
    super::outcome::answer(
        frame,
        (|| {
            let actor = crate::basis::room::conductor::current()
                .running_task()
                .ok_or(NestFail::Denied)?;
            let _serial = actor.pend.lock();
            if !pend::live(&actor) {
                return Err(NestFail::Dead);
            }
            if let NestCall::Delete { token } = call {
                binding::delete(&actor, token)?;
                return Ok(0);
            }
            let _commit = crate::basis::commit();
            match call {
                NestCall::Create { license, kind } => {
                    crate::basis::pend::access::license(&actor, license, CreationRights::NEST)
                        .map_err(|_| NestFail::Denied)?;
                    let nest = match kind {
                        NestKind::Internal => {
                            crate::basis::unit::team::TeamBuilder::prepare_nest(SpaceKind::User)
                        }
                        NestKind::External => Nest::external_empty(),
                    }
                    .map_err(|_| NestFail::OutOfMemory)?;
                    let root = Arc::try_new(
                        pend::PendEntry::try_new(
                            pend::Target::Nest {
                                owner: actor.ident.id,
                                nest: nest.clone(),
                            },
                            Mark::NONE,
                            Modifiable::Yes,
                            None,
                        )
                        .map_err(|_| NestFail::OutOfMemory)?,
                    )
                    .map_err(|_| NestFail::OutOfMemory)?;
                    let token: PendId = root.token();
                    let registration = actor
                        .pend
                        .prepare(root.clone())
                        .map_err(|_| NestFail::OutOfMemory)?;
                    if kind == NestKind::External {
                        binding::register(&nest, &actor, &root)?;
                    }
                    registration.publish();
                    pend::changed(&actor);
                    Ok(token.get())
                }
                NestCall::Inspect { token, out } => {
                    let info = binding::inspect(&actor, token)?;
                    if !ident.team.nest.copy_out(&info.to_bytes(), out) {
                        return Err(NestFail::Denied);
                    }
                    Ok(0)
                }
                NestCall::Attach { token, port } => {
                    binding::attach(&actor, token, port)?;
                    Ok(0)
                }
                NestCall::Detach { token } => {
                    binding::detach(&actor, token)?;
                    Ok(0)
                }
                NestCall::Delete { .. } => unreachable!(),
            }
        })(),
    );
}
