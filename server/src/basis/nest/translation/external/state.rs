//! Bounded hardware progress. Memo lends its publication; this state owns no Page.
use crate::basis::cell::page::{Change, Page, PageId};
use crate::basis::nest::PublicationError as Error;
use crate::basis::nest::memo::publication::{Phase, Publication};
use crate::metal::generic::iommu::{Fault, Hardware};
use alloc::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Admission {
    Open,
    Closing,
    Closed,
}

pub(crate) enum Progress {
    Pending,
    Published,
    Withdrawn,
    Closed,
}

pub(crate) struct External<H: Hardware<PageId>> {
    hardware: Option<H>,
    admission: Admission,
    error: Option<Error>,
}
impl<H: Hardware<PageId>> External<H> {
    pub(crate) const fn new() -> Self {
        Self {
            hardware: None,
            admission: Admission::Open,
            error: None,
        }
    }
    pub(crate) fn open(&self) -> bool {
        self.admission == Admission::Open
    }
    pub(crate) fn phase(&self) -> kernel_api::model::nest::NestPhase {
        use kernel_api::model::nest::NestPhase;
        if self.admission == Admission::Closed {
            return NestPhase::Closed;
        }
        if self.error.is_some() {
            return NestPhase::Failed;
        }
        match self.admission {
            Admission::Open if self.hardware.is_none() => NestPhase::Unbound,
            Admission::Open => NestPhase::Bound,
            Admission::Closing => NestPhase::Closing,
            Admission::Closed => NestPhase::Closed,
        }
    }
    pub(crate) fn settled(&self) -> bool {
        self.admission == Admission::Closed || self.hardware.is_none()
    }
    pub(crate) fn error(&self) -> Option<Error> {
        self.error
    }
    pub(crate) fn bind(&mut self, hardware: H) -> Result<(), (Error, H)> {
        if !self.open() || self.hardware.is_some() {
            return Err((Error::Closed, hardware));
        }
        self.hardware = Some(hardware);
        Ok(())
    }
    pub(crate) fn prepare(
        &self,
        occupied: bool,
        page: &Arc<Page>,
        address: usize,
        writable: bool,
        change: Option<(&Change, bool)>,
    ) -> Result<Publication, Error> {
        if !self.open() {
            return Err(Error::Closed);
        }
        if occupied {
            return Err(Error::Overlap);
        }
        let hardware = self.hardware.as_ref().ok_or(Error::UnknownDomain)?;
        if hardware.capacity() == 0 {
            return Err(Error::Capacity);
        }
        Publication::prepare_external(hardware, page, address, writable, change)
    }
    pub(crate) fn submit(&mut self, publication: Option<&mut Publication>) -> Result<(), Error> {
        if !self.open() {
            return Err(Error::Closed);
        }
        let result = publication
            .ok_or(Error::UnknownBinding)?
            .submit(self.hardware.as_mut().ok_or(Error::UnknownDomain)?);
        if let Err(error) = result {
            self.error = Some(error);
            self.close();
        }
        result
    }
    pub(crate) fn close(&mut self) {
        if self.open() {
            self.admission = Admission::Closing;
        }
    }
    pub(crate) fn fault(&mut self) -> Result<Option<Fault>, Error> {
        let result = self
            .hardware
            .as_mut()
            .ok_or(Error::UnknownDomain)?
            .fault()
            .map_err(Into::into);
        if let Err(error) = result {
            self.error = Some(error);
            self.close();
        }
        result
    }
    /// One bounded translation step and optional shutdown observation; neither waits.
    /// Completion never comes from a caller Ack.
    pub(crate) fn progress(
        &mut self,
        publication: Option<&mut Publication>,
        retracting: bool,
    ) -> Result<Progress, Error> {
        let result = self.step(publication, retracting);
        if let Err(error) = result {
            self.error = Some(error);
            self.close();
        }
        result
    }
    fn step(
        &mut self,
        publication: Option<&mut Publication>,
        retracting: bool,
    ) -> Result<Progress, Error> {
        if self.admission == Admission::Closed {
            return Ok(Progress::Closed);
        }
        let Some(hardware) = self.hardware.as_mut() else {
            if self.admission == Admission::Closing {
                self.admission = Admission::Closed;
            }
            return Ok(Progress::Pending);
        };
        if let Some(receipt) = publication {
            if self.admission == Admission::Closing || retracting {
                match receipt.phase() {
                    Phase::Waiting(_) => {
                        receipt.poll(hardware)?;
                    }
                    Phase::Settled => {}
                    _ => {
                        receipt.revoke(hardware)?;
                    }
                }
                if receipt.phase() == Phase::Settled {
                    return Ok(Progress::Withdrawn);
                }
            } else if receipt.phase() == Phase::Publishing && receipt.ready(hardware)? {
                return Ok(Progress::Published);
            }
        }
        Ok(Progress::Pending)
    }
    /// The owning Nest calls this only when every Memo has no publication.
    pub(crate) fn finish_close(&mut self) -> Result<bool, Error> {
        if self.admission == Admission::Closing {
            let done = match self.hardware.as_mut() {
                Some(hardware) => match hardware.shutdown() {
                    Ok(done) => done,
                    Err(error) => {
                        self.error = Some(error.into());
                        return Err(error.into());
                    }
                },
                None => true,
            };
            if done {
                self.admission = Admission::Closed;
            }
        }
        Ok(self.settled())
    }
}
impl<H: Hardware<PageId>> Drop for External<H> {
    fn drop(&mut self) {
        // An abandoned Nest must retain queues/tables as well as Memo's payload.
        // Only explicit shutdown can certify an installed backend as settled.
        if !self.settled() {
            core::mem::forget(self.hardware.take());
        }
    }
}
