use alloc::sync::Arc;

use kernel_api::abi::call::control::ControlCall;
use kernel_api::abi::call::control::ControlFail;

use crate::basis::nest::Access;
use crate::basis::unit::space::Space;
use crate::basis::unit::task::TaskIdent;
use crate::entry::report::backtrace;
use crate::entry::report::backtrace::Backtrace;
use crate::entry::report::backtrace::StackReader;
use crate::entry::report::backtrace::WalkConfig;
use crate::machine::switcher::context::Gprs;
use crate::machine::switcher::context::TrapContext;
use kernel_api::address::VirtAddr as KVirt;

use super::outcome::ret_err;

pub(super) fn dispatch(frame: &mut TrapContext, call: ControlCall, ident: &Arc<TaskIdent>) {
    match call {
        ControlCall::Backtrace { buf, frames } => {
            let space = &ident.team.space;
            let world = space.kind();
            let sp = frame.gpr.x(Gprs::SP);
            let fp = frame.gpr.x(Gprs::S0);
            let Some(len) = frames.checked_mul(size_of::<usize>()) else {
                ret_err(frame, ControlFail::Denied);
                return;
            };
            if !space.validate_access(KVirt::wrap(buf), len, Access::WRITE)
                || !Space::user_range(KVirt::wrap(sp), backtrace::SPAN)
            {
                ret_err(frame, ControlFail::Denied);
                return;
            }
            let mut reader = StackReader::user(ident.team.space.clone());
            let cfg = WalkConfig::normal(world, sp.saturating_add(backtrace::SPAN));
            let code = move |_w: usize| true;
            let trace = Backtrace::walk(&mut reader, &cfg, sp, fp, Some(&code));
            let pc_arr = trace.frames();
            let keep = pc_arr.len().min(frames);
            let mut bytes = [0u8; backtrace::DEPTH * ::core::mem::size_of::<usize>()];
            for i in 0..keep {
                bytes[i * ::core::mem::size_of::<usize>()..][..::core::mem::size_of::<usize>()]
                    .copy_from_slice(&pc_arr[i].pc.as_usize().to_le_bytes());
            }
            let kept = keep * ::core::mem::size_of::<usize>();
            let ok = space.copy_out(&bytes[..kept], KVirt::wrap(buf));
            if ok {
                frame.gpr.set_x(Gprs::A0, keep);
            } else {
                ret_err(frame, ControlFail::Denied);
            }
        }
    }
}
