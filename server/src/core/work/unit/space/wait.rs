//! The memory condition belongs to Space; Sign owns only its wait registration.
use super::Space;
use crate::core::memory::{address::VirtAddr, nest::NestError};
use crate::core::work::room::Handoff;
use crate::core::work::sign::{self, WakeKey, block_with_call, finish_block};
use crate::core::work::unit::task::WaitCall;
use crate::machine::clock;
use alloc::sync::Arc;
use core::time::Duration;
use kernel_api::abi::call::room::RoomFail;

pub(crate) fn wait_value(
    space: Arc<Space>,
    address: VirtAddr,
    expected: u32,
    duration: Duration,
) -> Result<Handoff<bool>, RoomFail> {
    let blocked = {
        let _commit = crate::core::work::commit();
        let value = space.nest.load_u32(address).map_err(|error| match error {
            NestError::Busy | NestError::Missing => RoomFail::Busy,
            _ => RoomFail::Denied,
        })?;
        if value != expected {
            return Ok(Handoff::Resume(true));
        }
        if duration == Duration::ZERO {
            return Ok(Handoff::Resume(false));
        }
        // Saturation prevents a very large finite timeout wrapping into the past.
        let deadline = (duration != Duration::MAX).then(|| {
            clock::now()
                .as_ticks()
                .saturating_add(clock::duration_to_ticks(duration))
        });
        let key = WakeKey::Value {
            space: space.nest.asid(),
            address: address.as_usize(),
        };
        let life = space.life();
        drop(space);
        match block_with_call::<RoomFail>(key, life, duration, Some(WaitCall::Value { deadline })) {
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
