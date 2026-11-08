//! A Page change closes admission atomically with retention registration.
//! A receipt is a unique ownership object. Arc only extends that same change
//! across calls and in-flight operations; the last holder reopens admission.
use super::super::Page;
use super::{Kind, RetainError, Retention};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static COMPLETED: AtomicBool = AtomicBool::new(false);
pub(crate) fn take_completion() -> bool {
    COMPLETED.swap(false, Ordering::AcqRel)
}

#[derive(Debug, PartialEq, Eq)]
enum Phase {
    Prepared,
    Held,
}

#[derive(Debug)]
pub(crate) struct Change {
    page: Arc<Page>,
    phase: Phase,
    freezes: AtomicUsize,
}
impl Change {
    pub(in super::super) fn begin(page: &Arc<Page>) -> Result<Arc<Self>, RetainError> {
        let mut change = Arc::try_new(Self {
            page: page.clone(),
            phase: Phase::Prepared,
            freezes: AtomicUsize::new(0),
        })
        .map_err(|_| RetainError::Exhausted)?;
        page.reclaim.begin()?;
        Arc::get_mut(&mut change).expect("unpublished change").phase = Phase::Held;
        Ok(change)
    }
    pub(crate) fn page(&self) -> &Arc<Page> {
        &self.page
    }
    pub(crate) fn hardware(&self, modifiable: bool) -> Result<Retention, RetainError> {
        if modifiable && self.freezes.load(Ordering::Acquire) > 1 {
            return Err(RetainError::Busy);
        }
        Retention::during(self, Kind::Cpu, modifiable)
    }
    pub(crate) fn external(&self, modifiable: bool) -> Result<Retention, RetainError> {
        if modifiable && self.freezes.load(Ordering::Acquire) > 1 {
            return Err(RetainError::Busy);
        }
        Retention::during(self, Kind::External, modifiable)
    }
}
impl Drop for Change {
    fn drop(&mut self) {
        if self.phase == Phase::Held {
            self.page.reclaim.end();
            COMPLETED.store(true, Ordering::Release);
        }
    }
}

/// A logical object's persistent freeze, distinct from a transient change permit.
#[derive(Debug)]
pub(crate) struct Freeze {
    change: Arc<Change>,
}
impl Freeze {
    pub(crate) fn new(change: Arc<Change>) -> Self {
        change.freezes.fetch_add(1, Ordering::AcqRel);
        Self { change }
    }
}
impl Drop for Freeze {
    fn drop(&mut self) {
        assert!(self.change.freezes.fetch_sub(1, Ordering::AcqRel) > 0);
    }
}
