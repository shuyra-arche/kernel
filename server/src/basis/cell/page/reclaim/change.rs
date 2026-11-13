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

struct Blocks {
    count: usize,
    next: usize,
    admitted: usize,
}

pub(crate) struct Change {
    page: Option<Arc<Page>>,
    phase: Phase,
    blocks: crate::metal::sync::SpinLock<Blocks>,
}
impl Change {
    pub(in super::super) fn begin(page: &Arc<Page>) -> Result<Arc<Self>, RetainError> {
        let mut change = Arc::try_new(Self {
            page: Some(page.clone()),
            phase: Phase::Prepared,
            blocks: crate::metal::sync::SpinLock::new(Blocks {
                count: 0,
                next: 0,
                admitted: 0,
            }),
        })
        .map_err(|_| RetainError::Exhausted)?;
        page.reclaim.begin()?;
        Arc::get_mut(&mut change).expect("unpublished change").phase = Phase::Held;
        Ok(change)
    }
    pub(crate) fn page(&self) -> &Arc<Page> {
        self.page.as_ref().expect("live Page change")
    }
    pub(crate) fn hardware(&self, modifiable: bool) -> Result<Retention, RetainError> {
        // CPU Memo publication holds its own freeze before preparing a PTE.
        let blocks = self.blocks.lock();
        if blocks.count != 0 {
            return Err(RetainError::Busy);
        }
        Retention::during(self, Kind::Cpu, modifiable, false)
    }
    pub(crate) fn external(&self, modifiable: bool) -> Result<Retention, RetainError> {
        // A device handoff can use the sole freeze left by CPU withdrawal.
        // External writers also require all CPU and other device use to settle.
        let blocks = self.blocks.lock();
        if blocks.count != 0 {
            return Err(RetainError::Busy);
        }
        Retention::during(self, Kind::External, modifiable, false)
    }
    /// Group verification and physical registration share the membership gate.
    pub(crate) fn qualified(
        &self,
        topology: &crate::metal::generic::memory::topology::Topology,
        endpoint: crate::metal::generic::memory::Endpoint,
        access: crate::metal::generic::memory::Access,
        maintenance: Option<&Maintenance>,
    ) -> Result<Retention, RetainError> {
        let blocks = self.blocks.lock();
        if let Some(permit) = maintenance {
            if !core::ptr::eq(self, permit.change.as_ref())
                || blocks.count != 1
                || blocks.admitted != permit.id
            {
                return Err(RetainError::Busy);
            }
        } else if blocks.count != 0 {
            return Err(RetainError::Busy);
        }
        let kind = match endpoint {
            crate::metal::generic::memory::Endpoint::Cpu { .. } => Kind::Cpu,
            crate::metal::generic::memory::Endpoint::External { .. } => Kind::External,
        };
        Retention::during_qualified(
            self,
            kind,
            access.writable(),
            maintenance.is_some(),
            Some((topology, endpoint, access)),
        )
    }
}
impl Drop for Change {
    fn drop(&mut self) {
        if self.phase == Phase::Held {
            self.page().reclaim.end();
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
    pub(crate) fn page(&self) -> &Arc<Page> {
        self.change.page()
    }
    pub(crate) fn new(change: Arc<Change>, retracts: Option<bool>) -> Result<Self, RetainError> {
        // Admission and the writer check are one transition. A prepared or
        // retiring writer still counts until its exact obligation discharges.
        change.page().reclaim.freeze(retracts)?;
        Ok(Self { change })
    }
}
impl Drop for Freeze {
    fn drop(&mut self) {
        self.change.page().reclaim.thaw();
    }
}

/// Blocks all new hardware retention, including users of this same Change.
/// Existing receipts must still discharge through their normal completion path.
pub(crate) struct Block {
    change: Arc<Change>,
    id: usize,
}
impl Block {
    /// Allocate an unpublished gate before an irreversible receipt conversion.
    pub(crate) fn prepare() -> Result<Self, RetainError> {
        Ok(Self {
            change: Arc::try_new(Change {
                page: None,
                phase: Phase::Prepared,
                blocks: crate::metal::sync::SpinLock::new(Blocks {
                    count: 1,
                    next: 1,
                    admitted: 1,
                }),
            })
            .map_err(|_| RetainError::Exhausted)?,
            id: 1,
        })
    }
    pub(crate) fn bind(&mut self, page: &Arc<Page>) {
        let change = Arc::get_mut(&mut self.change).expect("unpublished gate");
        assert!(change.page.is_none() && change.phase == Phase::Prepared);
        page.reclaim.begin().expect("fresh reshaped Page");
        change.page = Some(page.clone());
        change.phase = Phase::Held;
    }
    pub(crate) fn exclusive(&self, page: &Arc<Page>, registry_weak: usize) -> bool {
        Arc::ptr_eq(page, self.page())
            && self.settled()
            && Arc::strong_count(&self.change) == 1
            && Arc::weak_count(&self.change) == registry_weak
            && self.change.blocks.lock().count == 1
            && Arc::strong_count(page) == 2
            && Arc::weak_count(page) == 0
    }
    pub(crate) fn permit(&self) -> Result<Maintenance, RetainError> {
        let mut blocks = self.change.blocks.lock();
        if blocks.count != 1 {
            return Err(RetainError::Busy);
        }
        blocks.admitted = self.id;
        Ok(Maintenance {
            change: self.change.clone(),
            id: self.id,
        })
    }
    pub(crate) fn change(&self) -> &Arc<Change> {
        &self.change
    }

    /// Remove our own Page reference while retaining the closed admission gate.
    /// Arc::get_mut then atomically excludes every other strong/weak Page holder;
    /// refusal restores our reference without allocation or reopening admission.
    pub(crate) fn release_unique(&mut self, holding: &mut Arc<Page>) -> bool {
        if !Arc::ptr_eq(holding, self.page()) || !self.settled() {
            return false;
        }
        let Some(change) = Arc::get_mut(&mut self.change) else {
            return false;
        };
        if change.blocks.lock().count != 1 {
            return false;
        }
        drop(change.page.take());
        if let Some(page) = Arc::get_mut(holding) {
            page.reclaim.end();
            change.phase = Phase::Prepared;
            COMPLETED.store(true, Ordering::Release);
            true
        } else {
            change.page = Some(holding.clone());
            false
        }
    }
    pub(crate) fn new(change: Arc<Change>) -> Result<Self, RetainError> {
        let id = {
            let mut blocks = change.blocks.lock();
            let id = blocks.next.checked_add(1).ok_or(RetainError::Exhausted)?;
            let count = blocks.count.checked_add(1).ok_or(RetainError::Exhausted)?;
            blocks.next = id;
            blocks.count = count;
            blocks.admitted = id;
            id
        };
        Ok(Self { change, id })
    }
    pub(crate) fn page(&self) -> &Arc<Page> {
        self.change.page()
    }
    pub(crate) fn settled(&self) -> bool {
        self.change.page().reclaim.hardware_idle()
    }
}

/// A physical admission path derived from one retained Block. Logical ownership,
/// epoch and authority are rechecked by the range record under COMMIT. Every
/// physical registration still excludes additional overlapping Page blocks.
pub(crate) struct Maintenance {
    change: Arc<Change>,
    id: usize,
}
impl Maintenance {
    pub(crate) fn change(&self) -> &Arc<Change> {
        &self.change
    }
    pub(crate) fn hardware(&self, writable: bool) -> Result<Retention, RetainError> {
        let blocks = self.change.blocks.lock();
        if blocks.count != 1 || blocks.admitted != self.id {
            return Err(RetainError::Busy);
        }
        Retention::during(&self.change, Kind::Cpu, writable, true)
    }
    pub(crate) fn external(&self, writable: bool) -> Result<Retention, RetainError> {
        let blocks = self.change.blocks.lock();
        if blocks.count != 1 || blocks.admitted != self.id {
            return Err(RetainError::Busy);
        }
        Retention::during(&self.change, Kind::External, writable, true)
    }
}
impl Drop for Block {
    fn drop(&mut self) {
        let mut count = self.change.blocks.lock();
        assert!(count.count > 0);
        count.count -= 1;
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
