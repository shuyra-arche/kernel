use super::super::outcome::{Outcome, answer, ret_err};
use crate::{
    basis::{pend::access, unit::task::TaskIdent, view::clone::delivery},
    metal::switcher::context::{Gprs, TrapContext},
};
use alloc::sync::Arc;
use kernel_api::{abi::call::clone_view::CloneCall, model::view::ViewKind};
pub(in crate::entry::call) fn dispatch(
    frame: &mut TrapContext,
    call: CloneCall,
    ident: Arc<TaskIdent>,
) -> Outcome {
    match call {
        CloneCall::Create {
            license,
            mark,
            limits,
        } => answer(
            frame,
            delivery::create(license, mark, limits).map_err(delivery::fail),
        ),
        CloneCall::Delete { token } => answer(
            frame,
            access::seal_resource(token, ViewKind::Clone).map_err(delivery::fail),
        ),
        CloneCall::Push { token, msg, len } => answer(
            frame,
            delivery::push(ident, token, msg.get(), len).map(|()| 0),
        ),
        CloneCall::Pull { token, max_len } => match delivery::receive(token, max_len) {
            Ok(value) => {
                frame.gpr.set_x(Gprs::A0, value.token.get());
                frame.gpr.set_x(Gprs::A1, value.len);
                frame.gpr.set_x(Gprs::A2, value.sender.get());
            }
            Err(error) => ret_err(frame, error),
        },
        CloneCall::Peek { token } => match delivery::peek(token) {
            Ok((len, sender, depth)) => {
                frame.gpr.set_x(Gprs::A0, len);
                frame.gpr.set_x(Gprs::A1, sender.get());
                frame.gpr.set_x(Gprs::A2, depth);
            }
            Err(error) => ret_err(frame, error),
        },
    }
    Outcome::Resume
}
