//! The memory condition belongs to Nest; Sign owns only its wait registration.
use super::Nest;
use super::NestError;
use crate::basis::room::Handoff;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::sign::block_with_call;
use crate::basis::sign::finish_block;
use crate::basis::unit::task::WaitCall;
use crate::machine::clock;
use alloc::sync::Arc;
use core::time::Duration;
use kernel_api::abi::call::room::RoomFail;
use kernel_api::address::VirtAddr;

pub(crate) fn wait_value(
    space: Arc<Nest>,
    address: VirtAddr,
    expected: u32,
    wait: kernel_api::model::wait::Wait,
) -> Result<Handoff<bool>, RoomFail> {
    let deadline = clock::deadline(wait);
    let blocked = {
        let _commit = crate::basis::commit();
        let value = space.load_u32(address).map_err(|error| match error {
            NestError::Busy | NestError::Missing => RoomFail::Busy,
            _ => RoomFail::Denied,
        })?;
        if value != expected {
            return Ok(Handoff::Resume(true));
        }
        if clock::remaining(deadline) == Duration::ZERO {
            return Ok(Handoff::Resume(false));
        }
        let key = WakeKey::Value {
            space: space.id(),
            address: address.as_usize(),
        };
        let life = space.life();
        drop(space);
        match block_with_call::<RoomFail>(
            key,
            life,
            clock::remaining(deadline),
            Some(WaitCall::Value { deadline }),
        ) {
            Ok(blocked) => blocked,
            Err(error) => {
                // A timer/holder allocation may fail after creating the site.
                // Keep failed waits from accumulating empty address keys.
                sign::prune_empty(key);
                return Err(error);
            }
        }
    };
    Ok(match finish_block(blocked) {
        Handoff::Resume(()) => Handoff::Resume(true),
        Handoff::Switch(pa) => Handoff::Switch(pa),
    })
}
