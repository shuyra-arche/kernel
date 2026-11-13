//! Explicit residency commits. The caller holds the Flat state lock and owns
//! authorization/maintenance admission. All fallible preparation precedes
//! holding/revision changes. Legacy implicit sources cannot enter this path.
use super::{
    FlatSource, FlatState,
    slot::{self, Slot},
};
use crate::basis::cell::page::{Page, PageId};
use alloc::{sync::Arc, vec::Vec};
use kernel_api::{
    address::PageIndex,
    model::flat::{FlatVersion, SlotVersion},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Unsupported,
    Stale,
    Exhausted,
    OutOfMemory,
    Occupied,
    Empty,
    Busy,
}

pub(in super::super) struct Revision {
    current: FlatVersion,
    slots: Vec<(PageIndex, SlotVersion)>,
}
impl Revision {
    pub fn new() -> Self {
        Self {
            current: FlatVersion::INITIAL,
            slots: Vec::new(),
        }
    }
    pub fn current(&self) -> FlatVersion {
        self.current
    }
    pub fn at(&self, page: PageIndex) -> SlotVersion {
        self.slots
            .binary_search_by_key(&page.get(), |(p, _)| p.get())
            .ok()
            .map_or(SlotVersion::INITIAL, |at| self.slots[at].1)
    }
    fn prepare(&mut self, page: PageIndex, expected: SlotVersion) -> Result<(), Error> {
        if self.at(page) != expected {
            return Err(Error::Stale);
        }
        self.current.next_slots().ok_or(Error::Exhausted)?;
        if self
            .slots
            .binary_search_by_key(&page.get(), |(p, _)| p.get())
            .is_err()
        {
            self.slots.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        }
        Ok(())
    }
    fn commit(&mut self, page: PageIndex) -> SlotVersion {
        let (next, stamp) = self.current.next_slots().expect("prepared revision");
        self.current = next;
        match self
            .slots
            .binary_search_by_key(&page.get(), |(p, _)| p.get())
        {
            Ok(at) => self.slots[at].1 = stamp,
            Err(at) => self.slots.insert(at, (page, stamp)),
        }
        stamp
    }
}

impl FlatState {
    fn explicit(&self) -> Result<(), Error> {
        if !self.explicit || !self.transferable || !matches!(self.source, FlatSource::Managed(_)) {
            return Err(Error::Unsupported);
        }
        Ok(())
    }

    /// Takes a freshly allocated Page by value: retained Arc snapshots cannot
    /// be installed as a second holding. Missing is never accepted as Fresh.
    pub fn occupy(
        &mut self,
        index: PageIndex,
        expected: SlotVersion,
        page: Page,
    ) -> Result<(SlotVersion, PageId), Error> {
        self.explicit()?;
        if self.is_pinned(
            index,
            kernel_api::address::PageCount::new(page.pages()).unwrap(),
        ) {
            return Err(Error::Busy);
        }
        self.revision.prepare(index, expected)?;
        let FlatSource::Managed(slots) = &mut self.source else {
            unreachable!()
        };
        let (at, present) = slot::spot(slots, index);
        if present {
            return Err(match slots[at].1 {
                Slot::Missing => Error::Empty,
                Slot::Resident(_) => Error::Occupied,
                _ => Error::Busy,
            });
        }
        slots.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        let id = page.id();
        let page = Arc::try_new(page).map_err(|_| Error::OutOfMemory)?;
        slots.insert(at, (index, Slot::Resident(page)));
        let version = self.revision.commit(index);
        Ok((version, id))
    }

    /// Return the unique allocation outside the state lock. Reference counts
    /// establish only software exclusivity; Page's hardware accounting must
    /// independently permit return. Leaked/failed hardware receipts block this.
    pub fn vacate(
        &mut self,
        index: PageIndex,
        expected: SlotVersion,
    ) -> Result<(SlotVersion, Page), Error> {
        self.explicit()?;
        self.revision.prepare(index, expected)?;
        let FlatSource::Managed(slots) = &mut self.source else {
            unreachable!()
        };
        let (at, present) = slot::spot(slots, index);
        if !present {
            return Err(Error::Empty);
        }
        let page = match &mut slots[at].1 {
            Slot::Resident(page) => page,
            Slot::Missing => return Err(Error::Empty),
            _ => return Err(Error::Busy),
        };
        let end = index.get() + page.pages();
        if self
            .residencies
            .iter()
            .any(|r| r.index.get() < end && index.get() < r.index.get() + r.count.get())
        {
            return Err(Error::Busy);
        }
        if !page.allows_return() || Arc::get_mut(page).is_none() {
            return Err(Error::Busy);
        }
        let Slot::Resident(page) = core::mem::replace(&mut slots[at].1, Slot::Missing) else {
            unreachable!()
        };
        let page = Arc::try_unwrap(page).ok().expect("exclusive holding");
        let version = self.revision.commit(index);
        Ok((version, page))
    }
}
