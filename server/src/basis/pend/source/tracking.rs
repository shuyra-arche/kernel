//! Bounded discovery of sources that may still carry this Task's obligations.
//! Membership is not permission or a proof of retirement. Live entries are
//! removed only by the cleanup caller after its exact uses have settled.
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use kernel_api::abi::call::pend::PendFail;

pub(super) const CAPACITY: usize = 1024;

pub(super) struct Tracking<T> {
    sources: Vec<Weak<T>>,
}

impl<T> Tracking<T> {
    pub(super) const fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    pub(super) fn remember(&mut self, source: &Arc<T>) -> Result<(), PendFail> {
        self.sources.retain(|known| known.strong_count() != 0);
        if self
            .sources
            .iter()
            .any(|known| known.as_ptr() == Arc::as_ptr(source))
        {
            return Ok(());
        }
        if self.sources.len() >= CAPACITY {
            return Err(PendFail::OutOfMemory);
        }
        self.sources
            .try_reserve(1)
            .map_err(|_| PendFail::OutOfMemory)?;
        self.sources.push(Arc::downgrade(source));
        Ok(())
    }

    pub(super) fn next(&mut self) -> Option<Arc<T>> {
        // A source can lose its last strong reference during cleanup. Remove
        // only dead entries here, never a live source whose cleanup may fail.
        self.sources.retain(|known| known.strong_count() != 0);
        self.sources.iter().find_map(Weak::upgrade)
    }

    pub(super) fn forget(&mut self, source: &Arc<T>) {
        self.sources
            .retain(|known| known.as_ptr() != Arc::as_ptr(source));
    }
}
