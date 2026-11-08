//! Control-flow endpoints, pending notifications, source routing and waiting.

pub(crate) mod endpoint;
mod wait;

use wait::holder::holders;
use wait::site::SITE_SHARDS;
use wait::site::shard_at;

pub(crate) use wait::block::block_with_call;
pub(crate) use wait::block::finish_block;
pub(crate) use wait::detach;
pub(crate) use wait::forward;
pub(crate) use wait::holder::Ticket;
pub(crate) use wait::key::WakeKey;
pub(crate) use wait::knock;
pub(crate) use wait::park_until;
pub(crate) use wait::prune_empty;
pub(crate) use wait::redeem;
pub(crate) use wait::requeue;
pub(crate) use wait::signal;
pub(crate) use wait::unforward;
pub(crate) use wait::wait_until;
pub(crate) use wait::wake;
pub(crate) use wait::wake_counted;
pub(crate) use wait::wipe;
pub(crate) use wait::wipe_space;

pub(crate) fn clear_sources() {
    for shard in 0..SITE_SHARDS {
        let sources = core::mem::take(&mut *shard_at(shard).lock());
        drop(sources);
    }
}

pub(crate) fn clear_timers() {
    holders().lock().clear();
}

#[cfg(debug_assertions)]
pub(crate) fn timed_wait_count() -> usize {
    holders().lock().len()
}
