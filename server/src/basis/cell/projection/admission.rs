//! The Unit can close admission. The Kernel keeps a read-only watch until
//! hardware retirement completes, even after the Unit drops its handle.
use alloc::alloc::AllocError;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct Admission {
    watch: Watch,
}

impl Admission {
    pub(crate) fn new() -> Result<Self, AllocError> {
        Arc::try_new(AtomicBool::new(false)).map(|closed| Self {
            watch: Watch(closed),
        })
    }

    pub(crate) fn close(&self) {
        self.watch.0.store(true, Ordering::Release);
    }

    pub(crate) fn closed(&self) -> bool {
        self.watch.closed()
    }

    pub(crate) fn watch(&self) -> &Watch {
        &self.watch
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Clone)]
pub(crate) struct Watch(Arc<AtomicBool>);

impl Watch {
    pub(crate) fn closed(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
