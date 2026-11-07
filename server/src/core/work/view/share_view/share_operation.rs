use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use super::ShareView;

pub(crate) struct ShareOperation(Arc<ShareView>);

impl ShareOperation {
    pub(crate) fn acquire(meta: &Arc<ShareView>) -> Option<Self> {
        meta.operating
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;
        Some(Self(meta.clone()))
    }
}

impl Drop for ShareOperation {
    fn drop(&mut self) {
        self.0.operating.store(false, Ordering::Release);
    }
}
