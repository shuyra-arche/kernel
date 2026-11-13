//! Versioned residency of explicitly supplied Flats. Legacy implicit allocation
//! and transfer cannot mutate these sources. Authorization is checked by entry.
pub(crate) use super::storage::residency::Error;
use super::{Flat, FlatRange, FlatSource, Slot};
use crate::basis::cell::page::{Page, PageId};
use kernel_api::model::flat::{FlatVersion, SlotPhase, SlotStatus, SlotVersion};

impl Flat {
    pub(crate) fn phase(&self) -> kernel_api::model::flat::FlatPhase {
        self.state.lock().phase
    }
    pub(crate) fn begin_close(&self, expected: FlatVersion) -> Result<FlatVersion, Error> {
        self.state.lock().begin_close(expected)
    }
    pub(crate) fn finish_close(&self) -> Result<(), Error> {
        let released = self.state.lock().finish_close()?;
        drop(released);
        Ok(())
    }
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
    pub(crate) fn provenance(&self) -> Option<alloc::sync::Arc<crate::basis::pend::Provenance>> {
        self.flat()
            .state
            .lock()
            .provenance
            .iter()
            .find(|(at, _)| *at == self.index())
            .map(|(_, p)| p.clone())
    }
    pub(crate) fn staged(&self) -> bool {
        self.flat()
            .state
            .lock()
            .staged
            .iter()
            .any(|(at, _)| *at == self.index())
    }
    pub(crate) fn staged_by(
        &self,
        block: &alloc::sync::Arc<crate::basis::pend::RangeBlock>,
        page: PageId,
    ) -> bool {
        self.flat().state.lock().staged.iter().any(|(at, stage)| {
            *at == self.index()
                && stage.page == page
                && alloc::sync::Arc::ptr_eq(&stage.block, block)
        })
    }
    pub(crate) fn stage_origin(
        &self,
        block: &alloc::sync::Arc<crate::basis::pend::RangeBlock>,
        page: PageId,
    ) -> Option<usize> {
        self.flat()
            .state
            .lock()
            .staged
            .iter()
            .find(|(at, stage)| {
                *at == self.index()
                    && stage.page == page
                    && alloc::sync::Arc::ptr_eq(&stage.block, block)
            })
            .map(|(_, stage)| stage.origin)
    }
    pub(crate) fn slot_version(&self) -> Result<SlotVersion, Error> {
        let grain = self.grain();
        let state = self.flat().state.lock();
        if !state.explicit || self.page_count().get() != grain {
            return Err(Error::Unsupported);
        }
        Ok(state.revision.at(self.index()))
    }
    pub(crate) fn transfer_held(
        &self,
        expected: SlotVersion,
        target: &Self,
        target_version: SlotVersion,
        change: &alloc::sync::Arc<crate::basis::cell::page::Change>,
        stage: Option<alloc::sync::Arc<crate::basis::pend::Stage>>,
        provenance: Option<alloc::sync::Arc<crate::basis::pend::Provenance>>,
    ) -> Result<(SlotVersion, SlotVersion, PageId), Error> {
        if self.page_count().get() != self.grain()
            || target.page_count().get() != target.grain()
            || self.level() != target.level()
        {
            return Err(Error::Unsupported);
        }
        if alloc::sync::Arc::ptr_eq(self.flat(), target.flat()) {
            self.flat().state.lock().move_within_during(
                self.index(),
                expected,
                target.index(),
                target_version,
                Some(change),
                stage,
                provenance,
            )
        } else {
            crate::metal::sync::with_pair(&self.flat().state, &target.flat().state, |from, to| {
                from.move_to_during(
                    self.index(),
                    expected,
                    to,
                    target.index(),
                    target_version,
                    Some(change),
                    stage,
                    provenance,
                )
            })
        }
    }
    pub(crate) fn transfer_to(
        &self,
        expected: SlotVersion,
        target: &Self,
        target_version: SlotVersion,
        provenance: Option<alloc::sync::Arc<crate::basis::pend::Provenance>>,
    ) -> Result<(SlotVersion, SlotVersion, PageId), Error> {
        if self.page_count().get() != self.grain()
            || target.page_count().get() != target.grain()
            || self.level() != target.level()
        {
            return Err(Error::Unsupported);
        }
        if alloc::sync::Arc::ptr_eq(self.flat(), target.flat()) {
            self.flat().state.lock().move_within(
                self.index(),
                expected,
                target.index(),
                target_version,
                provenance,
            )
        } else {
            crate::metal::sync::with_pair(&self.flat().state, &target.flat().state, |from, to| {
                from.move_to(
                    self.index(),
                    expected,
                    to,
                    target.index(),
                    target_version,
                    provenance,
                )
            })
        }
    }
    pub(crate) fn inspect_slot(&self) -> Result<SlotStatus, Error> {
        let level = self.level();
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
        let (phase, page, obligations, blocked) = if !present {
            (SlotPhase::Fresh, 0, 0, false)
        } else {
            match &slots[at].1 {
                Slot::Resident(page) => (
                    SlotPhase::Resident,
                    page.id().get() as u64,
                    page.obligations(),
                    page.changing(),
                ),
                Slot::Missing => (SlotPhase::Missing, 0, 0, false),
                _ => (SlotPhase::Moving, 0, 0, false),
            }
        };
        Ok(SlotStatus {
            flat: self.source_id(),
            coordinate: self.index().get(),
            version: state.revision.at(self.index()),
            level,
            phase,
            page,
            obligations,
            blocked,
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
    pub(crate) fn vacate_held(
        &self,
        expected: SlotVersion,
        blocks: &mut alloc::vec::Vec<crate::basis::cell::page::Block>,
    ) -> Result<SlotVersion, Error> {
        if self.page_count().get() != self.grain() {
            return Err(Error::Unsupported);
        }
        let (stamp, page) =
            self.flat()
                .state
                .lock()
                .vacate(self.index(), expected, Some(blocks))?;
        drop(page);
        Ok(stamp)
    }
    pub(crate) fn reshape_held(
        &self,
        range: kernel_api::model::flat::Reshape,
        split: bool,
        expected: FlatVersion,
        blocks: &mut alloc::vec::Vec<crate::basis::cell::page::Block>,
    ) -> Result<(FlatVersion, SlotVersion), Error> {
        if range.offset() != self.index().get() || range.count() != self.page_count().get() {
            return Err(Error::Unsupported);
        }
        self.flat()
            .state
            .lock()
            .reshape(self.flat().level, range, split, expected, blocks)
    }
}
