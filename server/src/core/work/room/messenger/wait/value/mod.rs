use super::*;
use crate::core::memory::{address::VirtAddr, nest::NestError};
use crate::core::work::unit::space::Space;
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
        block_with_call::<RoomFail>(key, life, duration, Some(WaitCall::Value { deadline }))?
    };
    Ok(match finish_block(blocked) {
        Handoff::Resume(()) => Handoff::Resume(true),
        Handoff::Switch(pa) => Handoff::Switch(pa),
    })
}

pub(crate) fn wake_value(key: WakeKey, all: bool) -> usize {
    let _commit = crate::core::work::commit();
    let chain = {
        let mut table = sites(key).lock();
        let chain = table.get_mut(&key).and_then(|site| {
            if all {
                site.chain.take()
            } else {
                site.chain.pop()
            }
        });
        prune(&mut table, key);
        chain
    };
    let mut count = 0;
    for task in Unchain::new(chain) {
        count += 1;
        rise(core::iter::once(task));
    }
    count
}
