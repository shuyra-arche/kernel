use alloc::sync::Arc;

use kernel_api::abi::call::EnvCall;
use kernel_api::abi::call::debug::DebugCall;

use crate::basis::unit::task::TaskIdent;
use crate::machine::switcher::context::Gprs;
use crate::machine::switcher::context::TrapContext;

mod authority;
pub(in crate::entry) mod cell;
mod clone_view;
mod debug;
mod outcome;
mod pend;
mod room;
pub(crate) mod share_view;
pub(crate) mod sign;
mod unit;

use outcome::Outcome;
use outcome::ret_err;

/// 把这一次调用做掉，交回要接着跑的那一帧。
///
/// **不碰 `sepc`**：客人那条 `ebreak` 有多宽、之后从哪里续跑，是入口那一手的事
/// （`entry::trap::envcall`）——这里只管把这一格调用服务完，并把答案写进这一帧。
pub fn dispatch(frame: &mut TrapContext, ident: Arc<TaskIdent>) -> Option<*mut TrapContext> {
    let number = frame.gpr.x(Gprs::A7);
    let regs = [
        frame.gpr.x(Gprs::A0),
        frame.gpr.x(Gprs::A1),
        frame.gpr.x(Gprs::A2),
        frame.gpr.x(Gprs::A3),
        frame.gpr.x(Gprs::A4),
        frame.gpr.x(Gprs::A5),
    ];
    let envcall = match EnvCall::from_wire(number, &regs) {
        Ok(c) => c,
        Err(_) => {
            ret_err(frame, kernel_api::abi::error::DispatchFail::Unknown);
            return Some(frame as *mut TrapContext);
        }
    };
    let outcome = match envcall {
        EnvCall::Room(call) => room::dispatch(frame, call, ident),
        EnvCall::Unit(call) => unit::dispatch(frame, call, ident),
        EnvCall::Cell(call) => {
            cell::dispatch(frame, call, &ident);
            Outcome::Resume
        }
        EnvCall::Clone(call) => clone_view::dispatch(frame, call, ident),
        EnvCall::Share(call) => share_view::dispatch(frame, call, ident),
        EnvCall::Sign(call) => sign::dispatch(frame, call, ident),
        EnvCall::Pend(call) => {
            pend::dispatch(frame, call, ident);
            Outcome::Resume
        }
        EnvCall::Debug(DebugCall::Put { buf, len }) => {
            debug::put(frame, &ident, buf.get(), len);
            Outcome::Resume
        }
        EnvCall::Debug(DebugCall::Get { buf, len }) => {
            debug::get(frame, &ident, buf.get(), len);
            Outcome::Resume
        }
    };
    outcome.next(frame as *mut TrapContext)
}
