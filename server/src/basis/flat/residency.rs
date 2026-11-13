//! Versioned residency of explicitly supplied Flats. Legacy implicit allocation
//! and transfer cannot mutate these sources. Authorization is checked by entry.
pub(crate) use super::storage::residency::Error;
use super::{Flat, FlatRange, FlatSource, Slot};
use crate::basis::cell::page::{Page, PageId};
use kernel_api::{
    address::PageOffset,
    model::flat::{FlatVersion, SlotPhase, SlotStatus, SlotVersion},
};

impl Flat {
    pub(crate) fn is_explicit(&self) -> bool {
        self.state.lock().explicit
    }
    /// Only before the fresh source is registered or exposed to another owner.
    pub(crate) fn explicit(&mut self) {
        self.state.lock().explicit = true;
    }
    pub(crate) fn revision(&self) -> FlatVersion {
        self.state.lock().revision.current()
    }
}

impl FlatRange {
    pub(crate) fn inspect_slot(&self) -> Result<SlotStatus, Error> {
        if self.page_count().get() != self.grain() {
            return Err(Error::Unsupported);
        }
        let state = self.flat().state.lock();
        if !state.explicit {
            return Err(Error::Unsupported);
        }
        let FlatSource::Managed(slots) = &state.source else {
            return Err(Error::Unsupported);
        };
        let (at, present) = super::slot::spot(slots, self.index());
        let (phase, page) = if !present {
            (SlotPhase::Fresh, 0)
        } else {
            match &slots[at].1 {
                Slot::Resident(page) if page.allows_return() => {
                    (SlotPhase::Resident, page.id().get() as u64)
                }
                Slot::Resident(_) => return Err(Error::Busy),
                Slot::Missing => (SlotPhase::Missing, 0),
                _ => (SlotPhase::Moving, 0),
            }
        };
        Ok(SlotStatus {
            flat: self.source_id(),
            coordinate: self.index().get(),
            version: state.revision.at(self.index()),
            level: self.level(),
            phase,
            page,
            obligations: 0,
            blocked: false,
        })
    }

    /// Allocation/zeroing occurs before COMMIT; commit accepts only this fresh
    /// owned value, complete geometry and the still-current version.
    pub(crate) fn prepare_occupy(&self) -> Result<Page, super::FlatError> {
        crate::basis::cell::allocate_level(self.flat().allocation_owner.clone(), self.level())
    }
    pub(crate) fn occupy(
        &self,
        expected: SlotVersion,
        page: Page,
    ) -> Result<(SlotVersion, PageId), Error> {
        if self.page_count().get() != self.grain()
            || page.level() != self.level()
            || page.owner() != self.flat().allocation_owner.identity()
        {
            return Err(Error::Unsupported);
        }
        self.flat()
            .state
            .lock()
            .occupy(self.index(), expected, page)
    }
    pub(crate) fn vacate(&self, expected: SlotVersion) -> Result<SlotVersion, Error> {
        self.cell(PageOffset::ZERO)
            .map_err(|_| Error::Unsupported)?;
        if self.page_count().get() != self.grain() {
            return Err(Error::Unsupported);
        }
        let (stamp, page) = self.flat().state.lock().vacate(self.index(), expected)?;
        drop(page);
        Ok(stamp)
    }
}
