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
        publication: &mut Option<Publication>,
        page: &Arc<Page>,
        address: usize,
        writable: bool,
        change: Option<&Change>,
    ) -> Result<(), Error> {
        if !self.open() {
            return Err(Error::Closed);
        }
        if publication.is_some() {
            return Err(Error::Overlap);
        }
        let hardware = self.hardware.as_ref().ok_or(Error::UnknownDomain)?;
        if hardware.capacity() == 0 {
            return Err(Error::Capacity);
        }
        *publication = Some(Publication::prepare_external(
            hardware, page, address, writable, change,
        )?);
        Ok(())
    }
    pub(crate) fn submit(&mut self, publication: &mut Option<Publication>) -> Result<(), Error> {
        if !self.open() {
            return Err(Error::Closed);
        }
        let result = publication
            .as_mut()
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
        self.hardware
            .as_mut()
            .ok_or(Error::UnknownDomain)?
            .fault()
            .map_err(Into::into)
    }
    /// One bounded translation step and optional shutdown observation; neither waits.
    /// Completion never comes from a caller Ack.
    pub(crate) fn progress(
        &mut self,
        publication: &mut Option<Publication>,
    ) -> Result<Progress, Error> {
        let result = self.step(publication);
        self.error = result.as_ref().err().copied();
        if self.error.is_some() {
            self.close();
        }
        result
    }
    fn step(&mut self, publication: &mut Option<Publication>) -> Result<Progress, Error> {
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
            if self.admission == Admission::Closing {
                match receipt.phase() {
                    Phase::Waiting(_) => {
                        receipt.poll(hardware)?;
                    }
                    Phase::Settled => {}
                    _ => {
                        receipt.revoke(hardware)?;
                    }
                }
                if receipt.phase() == Phase::Settled && hardware.shutdown()? {
                    self.admission = Admission::Closed;
                    *publication = None;
                    return Ok(Progress::Withdrawn);
                }
            } else if receipt.phase() == Phase::Publishing && receipt.ready(hardware)? {
                return Ok(Progress::Published);
            }
        } else if self.admission == Admission::Closing && hardware.shutdown()? {
            self.admission = Admission::Closed;
            return Ok(Progress::Closed);
        }
        Ok(Progress::Pending)
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
