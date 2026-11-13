//! Explicit residency commits. The caller holds the Flat state lock and owns
//! authorization/maintenance admission. All fallible preparation precedes
//! holding/revision changes. Legacy implicit sources cannot enter this path.
use super::{
    FlatSource, FlatState,
    slot::{self, Slot},
};
use crate::basis::cell::page::{Block, Page, PageId, RetainError};
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

impl From<RetainError> for Error {
    fn from(error: RetainError) -> Self {
        match error {
            RetainError::Exhausted => Self::OutOfMemory,
            RetainError::Busy | RetainError::Borrowed | RetainError::Quarantined => Self::Busy,
            RetainError::Unbalanced => unreachable!("unbalanced hardware retention"),
        }
    }
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
        self.write(page, stamp);
        stamp
    }
    fn write(&mut self, page: PageIndex, stamp: SlotVersion) {
        match self
            .slots
            .binary_search_by_key(&page.get(), |(p, _)| p.get())
        {
            Ok(at) => self.slots[at].1 = stamp,
            Err(at) => self.slots.insert(at, (page, stamp)),
        }
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
        blocks: Option<&mut Vec<Block>>,
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
        if let Some(blocks) = blocks {
            let at = blocks
                .iter()
                .position(|block| Arc::ptr_eq(block.page(), page))
                .ok_or(Error::Busy)?;
            let block = blocks.remove(at);
            if let Err(block) = crate::basis::cell::change::release_block(block, page) {
                blocks.insert(at, block);
                return Err(Error::Busy);
            }
            assert!(
                page.allows_return() && Arc::get_mut(page).is_some(),
                "exclusive maintenance return"
            );
        }
        if !page.allows_return() || Arc::get_mut(page).is_none() {
            return Err(Error::Busy);
        }
        let Slot::Resident(page) = core::mem::replace(&mut slots[at].1, Slot::Missing) else {
            unreachable!()
        };
        let page = Arc::try_unwrap(page).ok().expect("exclusive holding");
        self.staged.retain(|(at, _)| *at != index);
        let version = self.revision.commit(index);
        Ok((version, page))
    }

    fn move_source(
        &mut self,
        index: PageIndex,
        expected: SlotVersion,
        maintenance: Option<&Arc<crate::basis::cell::page::Change>>,
    ) -> Result<Block, Error> {
        self.explicit()?;
        self.revision.prepare(index, expected)?;
        let FlatSource::Managed(slots) = &self.source else {
            unreachable!()
        };
        let (at, present) = slot::spot(slots, index);
        if !present {
            return Err(Error::Empty);
        }
        let page = match &slots[at].1 {
            Slot::Resident(page) => page,
            Slot::Missing => return Err(Error::Empty),
            _ => return Err(Error::Busy),
        };
        if self.is_pinned(
            index,
            kernel_api::address::PageCount::new(page.pages()).unwrap(),
        ) {
            return Err(Error::Busy);
        }
        // Close new hardware admission before inspecting old obligations. A
        // retained Arc may observe the old Page but cannot publish through this
        // gate. The first next-provider version does not move published Pages.
        let change = match maintenance {
            Some(change) if Arc::ptr_eq(change.page(), page) => change.clone(),
            Some(_) => return Err(Error::Busy),
            None => page.begin()?,
        };
        let block = Block::new(change)?;
        if !block.settled() {
            return Err(Error::Busy);
        }
        Ok(block)
    }
    fn move_target(
        &mut self,
        index: PageIndex,
        expected: SlotVersion,
        count: usize,
    ) -> Result<(), Error> {
        self.explicit()?;
        self.revision.prepare(index, expected)?;
        if self.is_pinned(index, kernel_api::address::PageCount::new(count).unwrap()) {
            return Err(Error::Busy);
        }
        let FlatSource::Managed(slots) = &mut self.source else {
            unreachable!()
        };
        let (at, present) = slot::spot(slots, index);
        if present {
            match slots[at].1 {
                Slot::Missing => {}
                Slot::Resident(_) => return Err(Error::Occupied),
                _ => return Err(Error::Busy),
            }
        } else {
            slots.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        }
        Ok(())
    }
    fn take_holding(&mut self, index: PageIndex) -> Arc<Page> {
        let FlatSource::Managed(slots) = &mut self.source else {
            unreachable!()
        };
        let (at, present) = slot::spot(slots, index);
        assert!(present);
        let Slot::Resident(page) = core::mem::replace(&mut slots[at].1, Slot::Missing) else {
            unreachable!()
        };
        page
    }
    fn put_holding(&mut self, index: PageIndex, page: Arc<Page>) {
        let FlatSource::Managed(slots) = &mut self.source else {
            unreachable!()
        };
        let (at, present) = slot::spot(slots, index);
        if present {
            slots[at].1 = Slot::Resident(page);
        } else {
            slots.insert(at, (index, Slot::Resident(page)));
        }
    }
    /// Both Flat locks and the caller's authorization commit remain held.
    pub fn move_to(
        &mut self,
        from: PageIndex,
        expected: SlotVersion,
        target: &mut Self,
        to: PageIndex,
        target_version: SlotVersion,
    ) -> Result<(SlotVersion, SlotVersion, PageId), Error> {
        self.move_to_during(from, expected, target, to, target_version, None, None)
    }
    pub fn move_to_during(
        &mut self,
        from: PageIndex,
        expected: SlotVersion,
        target: &mut Self,
        to: PageIndex,
        target_version: SlotVersion,
        maintenance: Option<&Arc<crate::basis::cell::page::Change>>,
        stage: Option<Arc<crate::basis::pend::Stage>>,
    ) -> Result<(SlotVersion, SlotVersion, PageId), Error> {
        let block = self.move_source(from, expected, maintenance)?;
        target.move_target(to, target_version, block.page().pages())?;
        if stage.is_some() {
            target
                .staged
                .try_reserve(1)
                .map_err(|_| Error::OutOfMemory)?;
        }
        let id = block.page().id();
        let page = self.take_holding(from);
        target.put_holding(to, page);
        self.staged.retain(|(at, _)| *at != from);
        if let Some(stage) = stage {
            target.staged.push((to, stage));
        }
        let source_version = self.revision.commit(from);
        let target_version = target.revision.commit(to);
        Ok((source_version, target_version, id))
    }
    /// One Flat advances once and gives both changed heads the same stamp.
    pub fn move_within(
        &mut self,
        from: PageIndex,
        expected: SlotVersion,
        to: PageIndex,
        target_version: SlotVersion,
    ) -> Result<(SlotVersion, SlotVersion, PageId), Error> {
        self.move_within_during(from, expected, to, target_version, None, None)
    }
    pub fn move_within_during(
        &mut self,
        from: PageIndex,
        expected: SlotVersion,
        to: PageIndex,
        target_version: SlotVersion,
        maintenance: Option<&Arc<crate::basis::cell::page::Change>>,
        stage: Option<Arc<crate::basis::pend::Stage>>,
    ) -> Result<(SlotVersion, SlotVersion, PageId), Error> {
        if from == to {
            return Err(Error::Occupied);
        }
        let block = self.move_source(from, expected, maintenance)?;
        if stage.is_some() {
            self.staged.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        }
        self.move_target(to, target_version, block.page().pages())?;
        self.revision
            .slots
            .try_reserve(2)
            .map_err(|_| Error::OutOfMemory)?;
        let id = block.page().id();
        let page = self.take_holding(from);
        self.put_holding(to, page);
        self.staged.retain(|(at, _)| *at != from);
        if let Some(stage) = stage {
            self.staged.push((to, stage));
        }
        let stamp = self.revision.commit(from);
        self.revision.write(to, stamp);
        Ok((stamp, stamp, id))
    }
}
