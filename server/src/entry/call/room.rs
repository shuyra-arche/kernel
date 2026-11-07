use ::core::time::Duration;

use alloc::sync::Arc;

use kernel_api::{abi::call::room::RoomCall, abi::call::room::RoomFail};

use crate::core::work::room::Handoff;
use crate::core::work::room::conductor::current;
use crate::core::work::room::scheduler::roster::muster;
use crate::core::work::sign::{self, WakeKey, park, park_until, wait, wake};
use crate::core::work::unit::task::TaskIdent;
use crate::machine::switcher::context::{Gprs, TrapContext};

use super::outcome::Outcome;

pub(super) fn dispatch(frame: &mut TrapContext, call: RoomCall, ident: Arc<TaskIdent>) -> Outcome {
    match call {
        RoomCall::Starve => {
            drop(ident);
            Outcome::Switch(current().starve() as *mut TrapContext)
        }
        RoomCall::Reap { reason, note, len } => {
            crate::core::work::unit::lifecycle::set_exit_note(note.get(), len);
            crate::core::work::unit::lifecycle::set_exit_reason(reason);
            drop(ident);
            Outcome::Exit
        }
        RoomCall::Doom { task } => {
            let Some(me) = muster(ident.id).and_then(|w| w.upgrade()) else {
                return Outcome::fail(frame, RoomFail::Denied);
            };
            if !Arc::ptr_eq(&me.ident, &ident)
                || !me
                    .administration
                    .allows(kernel_api::model::unit::ControlKind::Doom)
            {
                return Outcome::fail(frame, RoomFail::Denied);
            }
            drop(me);
            let target = muster(task).and_then(|w| w.upgrade());
            let Some(target) = target else {
                return Outcome::fail(frame, RoomFail::Dead);
            };
            let team = target.ident.team.clone();
            if team.all_reaped() {
                return Outcome::fail(frame, RoomFail::Dead);
            }
            drop(target);
            drop(ident);
            crate::core::work::unit::lifecycle::cull(
                &[team],
                crate::core::work::unit::lifecycle::EXIT_DOOM,
            );
            frame.gpr.set_x(Gprs::A0, 0);
            Outcome::Resume
        }
        RoomCall::Park { millis } => {
            drop(ident);
            match park::<RoomFail>(Duration::from_millis(millis as u64)) {
                Ok(pa) => Outcome::Switch(pa as *mut TrapContext),
                Err(e) => Outcome::fail(frame, e),
            }
        }
        RoomCall::ParkUntil { at } => {
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
            millis,
        } => {
            let at = crate::core::memory::address::VirtAddr::from_raw(address.get());
            if address.get() % 4 != 0 || !crate::core::work::unit::space::Space::user_range(at, 4) {
                return Outcome::fail(frame, RoomFail::Invalid);
            }
            let space = ident.team.space.clone();
            drop(ident);
            match crate::core::work::unit::space::wait::wait_value(
                space,
                at,
                expected,
                millis.into_duration(),
            ) {
                Ok(Handoff::Resume(value)) => {
                    frame.gpr.set_x(Gprs::A0, value as usize);
                    Outcome::Resume
                }
                Ok(Handoff::Switch(pa)) => Outcome::Switch(pa as *mut TrapContext),
                Err(error) => Outcome::fail(frame, error),
            }
        }
        RoomCall::WakeValue { address, all } => {
            let at = crate::core::memory::address::VirtAddr::from_raw(address.get());
            if address.get() % 4 != 0 || !crate::core::work::unit::space::Space::user_range(at, 4) {
                return Outcome::fail(frame, RoomFail::Invalid);
            }
            let key = WakeKey::Value {
                space: ident.team.space.nest.asid(),
                address: address.get(),
            };
            frame.gpr.set_x(Gprs::A0, sign::wake_counted(key, all));
            Outcome::Resume
        }
        RoomCall::Wait { key, millis } => {
            let (space, wlife) = {
                let s = &ident.team.space;
                (s.nest.asid(), s.life())
            };
            let wkey = WakeKey::Space { space, slot: key };
            let dur = millis.into_duration();
            drop(ident);
            match wait::<RoomFail>(wkey, wlife, dur) {
                Ok(Handoff::Resume(())) => Outcome::Resume,
                Ok(Handoff::Switch(pa)) => Outcome::Switch(pa as *mut TrapContext),
                Err(e) => Outcome::fail(frame, e),
            }
        }
        RoomCall::Wake { key } => {
            let (space, wlife) = {
                let s = &ident.team.space;
                (s.nest.asid(), s.life())
            };
            let wkey = WakeKey::Space { space, slot: key };
            let woke = wake(wkey, &wlife);
            frame.gpr.set_x(Gprs::A0, woke as usize);
            Outcome::Resume
        }
    }
}
