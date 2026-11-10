//! A Page change closes admission atomically with retention registration.
//! A receipt is a unique ownership object. Arc only extends that same change
//! across calls and in-flight operations; the last holder reopens admission.
use super::super::Page;
use super::{Kind, RetainError, Retention};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

static COMPLETED: AtomicBool = AtomicBool::new(false);
pub(crate) fn take_completion() -> bool {
    COMPLETED.swap(false, Ordering::AcqRel)
}

#[derive(Debug, PartialEq, Eq)]
enum Phase {
    Prepared,
    Held,
}

pub(crate) struct Change {
    page: Arc<Page>,
    phase: Phase,
    blocks: crate::metal::sync::SpinLock<usize>,
}
impl Change {
    pub(in super::super) fn begin(page: &Arc<Page>) -> Result<Arc<Self>, RetainError> {
        let mut change = Arc::try_new(Self {
            page: page.clone(),
            phase: Phase::Prepared,
            blocks: crate::metal::sync::SpinLock::new(0),
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
        // CPU Memo publication holds its own freeze before preparing a PTE.
        let blocks = self.blocks.lock();
        if *blocks != 0 {
            return Err(RetainError::Busy);
        }
        Retention::during(self, Kind::Cpu, modifiable)
    }
    pub(crate) fn external(&self, modifiable: bool) -> Result<Retention, RetainError> {
        // A device handoff can use the sole freeze left by CPU withdrawal.
        // External writers also require all CPU and other device use to settle.
        let blocks = self.blocks.lock();
        if *blocks != 0 {
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
    pub(crate) fn new(change: Arc<Change>, retracts: Option<bool>) -> Result<Self, RetainError> {
        // Admission and the writer check are one transition. A prepared or
        // retiring writer still counts until its exact obligation discharges.
        change.page.reclaim.freeze(retracts)?;
        Ok(Self { change })
    }
}
impl Drop for Freeze {
    fn drop(&mut self) {
        self.change.page.reclaim.thaw();
    }
}

/// Blocks all new hardware retention, including users of this same Change.
/// Existing receipts must still discharge through their normal completion path.
pub(crate) struct Block {
    change: Arc<Change>,
}
impl Block {
    pub(crate) fn new(change: Arc<Change>) -> Result<Self, RetainError> {
        {
            let mut count = change.blocks.lock();
            *count = count.checked_add(1).ok_or(RetainError::Exhausted)?;
        }
        Ok(Self { change })
    }
    pub(crate) fn page(&self) -> &Arc<Page> {
        self.change.page()
    }
    pub(crate) fn settled(&self) -> bool {
        self.change.page.reclaim.hardware_idle()
    }
}
impl Drop for Block {
    fn drop(&mut self) {
        let mut count = self.change.blocks.lock();
        assert!(*count > 0);
        *count -= 1;
    }
}

impl core::fmt::Debug for Change {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Change")
            .field("page", &self.page)
            .field("phase", &self.phase)
            .finish_non_exhaustive()
    }
}
