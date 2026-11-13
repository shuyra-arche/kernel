use super::super::outcome::{Outcome, answer, answer_pair};
use crate::{
    basis::{
        pend::access as authority,
        room::scheduler::slot::current,
        unit::task::TaskIdent,
        view::share::lease::{create_share, open_resource, shut_resource},
    },
    metal::switcher::context::TrapContext,
};
use alloc::sync::Arc;
use kernel_api::{
    abi::call::{
        pend::PendFail,
        share_view::{ShareCall, ShareFail},
    },
    model::view::ViewKind,
};
pub(in crate::entry::call) fn dispatch(
    frame: &mut TrapContext,
    call: ShareCall,
    ident: Arc<TaskIdent>,
) -> Outcome {
    match call {
        ShareCall::Derive { license, request } => {
            let result = (|| {
                use kernel_api::model::pend::{
                    AccordRequest, AccordTarget, CreationRights, Operations,
                };
                let task = current().running_task().ok_or(PendFail::Denied)?;
                let mut bytes = [0; AccordRequest::WIDTH];
                if !ident.team.nest.copy_in(&mut bytes, request) {
                    return Err(PendFail::Denied);
                }
                let request = AccordRequest::from_bytes(&bytes).ok_or(PendFail::Invalid)?;
                if !matches!(request.limits.operations, Operations::Share(_)) {
                    return Err(PendFail::Denied);
                }
                let target = match request.target {
                    AccordTarget::Caller => Arc::downgrade(&task),
                    AccordTarget::Task(id) => crate::basis::unit::task::registry::find(id)
                        .ok_or(PendFail::Denied)?
                        .as_weak()
                        .clone(),
                };
                crate::basis::pend::accord(
                    &task,
                    request.src,
                    &target,
                    request.limits,
                    request.mark,
                    crate::basis::pend::Window::Parent(
                        request.limits.scope.range().ok_or(PendFail::Denied)?,
                    ),
                    Some((license, Operations::None(CreationRights::SHARE))),
                )
            })();
            answer(frame, result.map_err(fail));
        }
        ShareCall::Create {
            license,
            reference,
            modifiable,
        } => {
            let result = current()
                .running_task()
                .ok_or(PendFail::Denied)
                .and_then(|task| create_share(&task, license, reference, modifiable));
            answer(frame, result.map(|token| token.get()).map_err(fail));
        }
        ShareCall::Delete { token } => {
            answer(
                frame,
                authority::seal_resource(token, ViewKind::Share).map_err(fail),
            );
        }
        ShareCall::Open { token } => {
            answer_pair(frame, open_resource(ident, token).map_err(fail));
        }
        ShareCall::Shut { token } => {
            answer(frame, shut_resource(token).map_err(fail));
        }
    }
    Outcome::Resume
}

fn fail(error: PendFail) -> ShareFail {
    match error {
        PendFail::Denied | PendFail::Invalid => ShareFail::Denied,
        PendFail::NotAligned => ShareFail::NotAligned,
        PendFail::Dead => ShareFail::Dead,
        PendFail::OutOfMemory => ShareFail::OutOfMemory,
        PendFail::HandedOver => ShareFail::HandedOver,
        PendFail::Busy => ShareFail::Busy,
    }
}
