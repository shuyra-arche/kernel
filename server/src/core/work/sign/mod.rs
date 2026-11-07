//! Control-flow notification, waiting and source routing.
//!
//! This first migration keeps existing source keys and notification policies.
//! Unit owns lifecycle/results; a task owns its suspended continuation. Sign
//! only delivers opportunities to resume and does not interpret result data.
//! Public Sign handles and reliable event queues are separate follow-up work.

mod wait;

use wait::holder::holders;
use wait::site::{SITE_SHARDS, shard_at};

pub(crate) use wait::block::block_with_call;
pub(crate) use wait::blocked::finish_block;
pub(crate) use wait::holder::Ticket;
pub(crate) use wait::key::WakeKey;
pub(crate) use wait::{
    detach, fall, forward, knock, park, park_until, redeem, requeue, signal, unforward, wait,
    wait_until, wake, wipe, wipe_space,
};
pub(crate) use wait::{prune_empty, wake_counted};

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
