use super::payload::Payload;
use alloc::sync::Arc;

use kernel_api::model::identity::TaskId;

pub(super) struct Hand {
    pub(super) from: TaskId,
    pub(super) buf: Arc<Payload>,
    pub(super) at: u64,
}
