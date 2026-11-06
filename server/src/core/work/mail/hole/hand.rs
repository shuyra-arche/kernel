use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::model::identity::TaskId;

pub(super) struct Hand {
    pub(super) from: TaskId,
    pub(super) buf: Arc<Vec<u8>>,
    pub(super) at: u64,
}
