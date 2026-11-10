//! Test assembly of the production hardware state and Memo publication slot.
//! No authority, address registry, retirement state machine or completion is simulated here.
use crate::cell::{Change, Page, PageId};
use crate::external::{External, Progress};
use crate::iommu::{BatchId, Error, Fault, Hardware};
use crate::publication::{Phase, Publication};
use alloc::sync::Arc;

pub(crate) struct Target<H: Hardware<PageId>> {
    backend: External<H>,
    publication: Option<Publication>,
}
impl<H: Hardware<PageId>> Target<H> {
    pub(crate) fn new(hardware: H) -> Self {
        let mut backend = External::new();
        backend.bind(hardware).map_err(|(e, _)| e).unwrap();
        Self {
            backend,
            publication: None,
        }
    }
    pub(crate) fn prepare(
        &mut self,
        page: &Arc<Page>,
        address: usize,
        writable: bool,
        change: Option<&Change>,
    ) -> Result<(), Error> {
        let publication =
            self.backend
                .prepare(self.publication.is_some(), page, address, writable, change)?;
        self.publication = Some(publication);
        Ok(())
    }
    pub(crate) fn publish(&mut self) -> Result<(), Error> {
        self.backend.submit(self.publication.as_mut())
    }
    pub(crate) fn phase(&self) -> Option<Phase> {
        self.publication.as_ref().map(Publication::phase)
    }
    pub(crate) fn ready(&mut self) -> Result<bool, Error> {
        if !self.backend.open() {
            return Err(Error::Closed);
        }
        self.progress()?;
        Ok(self.phase() == Some(Phase::Mapped))
    }
    pub(crate) fn fault(&mut self) -> Result<Option<Fault>, Error> {
        self.backend.fault()
    }
    pub(crate) fn revoke(&mut self) -> Result<Option<BatchId>, Error> {
        self.backend.close();
        // Production progress is one step; don't poll an already submitted batch here.
        if let Some(Phase::Waiting(batch)) = self.phase() {
            return Ok(Some(batch));
        }
        self.progress()?;
        Ok(match self.phase() {
            Some(Phase::Waiting(batch)) => Some(batch),
            _ => None,
        })
    }
    pub(crate) fn poll(&mut self) -> Result<bool, Error> {
        if self.publication.is_none() {
            return Err(Error::UnknownBinding);
        }
        Ok(matches!(self.progress()?, Progress::Withdrawn))
    }
    pub(crate) fn advance_close(&mut self) -> Result<bool, Error> {
        self.backend.close();
        self.progress()?;
        if self.publication.is_none() {
            self.backend.finish_close()?;
        }
        Ok(self.backend.settled())
    }
    fn progress(&mut self) -> Result<Progress, Error> {
        let progress = self.backend.progress(self.publication.as_mut(), false)?;
        if matches!(progress, Progress::Withdrawn) {
            self.publication = None;
        }
        Ok(progress)
    }
    pub(crate) fn settled(&self) -> bool {
        self.backend.settled()
    }
}
