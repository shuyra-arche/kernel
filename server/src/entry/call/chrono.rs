use kernel_api::abi::call::chrono::ChronoCall;

use crate::basis::work::room::deadline;
use crate::machine::clock;
use crate::machine::switcher::context::{Gprs, TrapContext};

pub(super) fn dispatch(frame: &mut TrapContext, call: ChronoCall) {
    match call {
        ChronoCall::Ticks => {
            frame.gpr.set_x(Gprs::A0, deadline::ticks() as usize);
        }
        ChronoCall::Clock => {
            let ns = clock::uptime().as_nanos().min(u64::MAX as u128) as u64;
            frame.gpr.set_x(Gprs::A0, ns as usize);
        }
    }
}
