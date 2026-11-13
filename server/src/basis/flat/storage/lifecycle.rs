//! Irreversible logical closure and exclusive physical return.
use super::{FlatSource, FlatState, Slot, residency::Error};
use kernel_api::model::flat::FlatVersion;
impl FlatState {
    pub fn begin_close(&mut self, expected: FlatVersion) -> Result<FlatVersion, Error> {
        use kernel_api::model::flat::FlatPhase;
        if !self.explicit {
            return Err(Error::Unsupported);
        }
        if self.revision.current() != expected {
            return Err(Error::Stale);
        }
        if self.phase == FlatPhase::Open {
            self.revision.advance()?;
            self.phase = FlatPhase::Closing;
        }
        Ok(self.revision.current())
    }
    /// Admission is already sealed. No holding is dropped until the entire
    /// allocation set is exclusive and every physical responsibility settled.
    pub fn finish_close(&mut self) -> Result<FlatSource, Error> {
        use kernel_api::model::flat::FlatPhase;
        if self.phase == FlatPhase::Closed {
            return Ok(FlatSource::Managed(alloc::vec::Vec::new()));
        }
        if self.phase != FlatPhase::Closing
            || !self.residencies.is_empty()
            || !self.staged.is_empty()
        {
            return Err(Error::Busy);
        }
        let FlatSource::Managed(slots) = &mut self.source else {
            return Err(Error::Unsupported);
        };
        for (_, slot) in slots.iter_mut() {
            match slot {
                Slot::Missing => {}
                Slot::Resident(page) => {
                    if alloc::sync::Arc::get_mut(page).is_none_or(|page| !page.allows_return()) {
                        return Err(Error::Busy);
                    }
                }
                _ => return Err(Error::Busy),
            }
        }
        let released = FlatSource::Managed(core::mem::take(slots));
        self.provenance.clear();
        self.phase = FlatPhase::Closed;
        Ok(released)
    }
}
