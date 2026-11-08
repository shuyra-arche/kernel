use super::outcome::Outcome;
use crate::basis::room::{Handoff, conductor::current};
use crate::basis::sign::{self, WakeKey, park_until};
use crate::basis::unit::task::TaskIdent;
use crate::metal::switcher::context::{Gprs, TrapContext};
use alloc::sync::Arc;
use kernel_api::abi::call::room::{RoomCall, RoomFail};
pub(super) fn dispatch(frame: &mut TrapContext, call: RoomCall, ident: Arc<TaskIdent>) -> Outcome {
    frame.gpr.set_x(Gprs::A0, 0);
    match call {
        RoomCall::Yield => {
            drop(ident);
            Outcome::Switch(current().starve() as *mut TrapContext)
        }
        RoomCall::Clock => {
            frame.gpr.set_x(
                Gprs::A0,
                crate::metal::clock::uptime()
                    .as_nanos()
                    .min(u64::MAX as u128) as usize,
            );
            Outcome::Resume
        }
        RoomCall::ClockInfo => {
            let Some(source) = crate::metal::clock::user_source() else {
                return Outcome::fail(frame, RoomFail::Denied);
            };
            frame.gpr.set_x(Gprs::A1, source.boot_tick() as usize);
            frame.gpr.set_x(Gprs::A2, source.hertz().get() as usize);
            Outcome::Resume
        }
        RoomCall::SleepUntil { at } => {
            drop(ident);
            match park_until::<RoomFail>(at) {
                Ok(None) => Outcome::Resume,
                Ok(Some(pa)) => Outcome::Switch(pa as *mut TrapContext),
                Err(e) => Outcome::fail(frame, e),
            }
        }
        RoomCall::WaitValue {
            address,
            expected,
            wait,
        } => {
            let at = crate::metal::mmu::canonical(address.get());
            if address.get() % 4 != 0 || !crate::basis::nest::Nest::user_range(at, 4) {
                return Outcome::fail(frame, RoomFail::Invalid);
            }
            let space = ident.team.nest.clone();
            drop(ident);
            match crate::basis::nest::wait::wait_value(space, at, expected, wait) {
                Ok(Handoff::Resume(value)) => {
                    frame.gpr.set_x(Gprs::A0, value as usize);
                    Outcome::Resume
                }
                Ok(Handoff::Switch(pa)) => Outcome::Switch(pa as *mut TrapContext),
                Err(error) => Outcome::fail(frame, error),
            }
        }
        RoomCall::WakeValue { address, all } => {
            let at = crate::metal::mmu::canonical(address.get());
            if address.get() % 4 != 0 || !crate::basis::nest::Nest::user_range(at, 4) {
                return Outcome::fail(frame, RoomFail::Invalid);
            }
            let key = WakeKey::Value {
                space: ident.team.nest.id(),
                address: address.get(),
            };
            frame.gpr.set_x(Gprs::A0, sign::wake_counted(key, all));
            Outcome::Resume
        }
    }
}
