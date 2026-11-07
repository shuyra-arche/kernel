//! A Page change closes admission atomically with retention registration.
//! A receipt is a unique ownership object. Arc only extends that same change
//! across calls and in-flight operations; the last holder reopens admission.
use super::reclaim::Kind;
use super::{Page, RetainError, Retention};
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
    id: usize,
    phase: Phase,
}
impl Change {
    pub(super) fn begin(page: &Arc<Page>) -> Result<Arc<Self>, RetainError> {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_add(1).filter(|next| *next <= isize::MAX as usize)
            })
            .map_err(|_| RetainError::Exhausted)?;
        let mut change = Arc::try_new(Self {
            page: page.clone(),
            id,
            phase: Phase::Prepared,
        })
        .map_err(|_| RetainError::Exhausted)?;
        page.reclaim.begin()?;
        Arc::get_mut(&mut change).expect("unpublished change").phase = Phase::Held;
        Ok(change)
    }
    pub(crate) fn id(&self) -> usize {
        self.id
    }
    pub(crate) fn page(&self) -> &Arc<Page> {
        &self.page
    }
    pub(crate) fn hardware(&self, modifiable: bool) -> Result<Retention, RetainError> {
        Retention::during(self, Kind::Cpu, modifiable)
    }
    pub(crate) fn external(&self, modifiable: bool) -> Result<Retention, RetainError> {
        Retention::during(self, Kind::Device, modifiable)
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
