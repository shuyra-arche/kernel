//! One publication obligation, shared by CPU and external translations.
use crate::basis::cell::page::{Change, Page, PageId, RetainError, Retention};
use crate::basis::nest::PublicationError as Error;
use crate::metal::generic::iommu::{BatchId, BindingId, Hardware, IoAddress, Projection};
use alloc::sync::Arc;
use core::ops::Deref;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Prepared,
    Publishing,
    Mapped,
    Withdrawing,
    Waiting(BatchId),
    Failed,
    Settled,
}

pub(crate) struct Publication {
    retention: Option<Retention>,
    phase: Phase,
    external: Option<Projection<PageId>>,
}
impl Publication {
    pub(crate) fn prepare(
        page: &Arc<Page>,
        writable: bool,
        change: Option<&Change>,
    ) -> Result<Self, RetainError> {
        let retention = match change {
            Some(change) if Arc::ptr_eq(change.page(), page) => change.hardware(writable)?,
            Some(_) => return Err(RetainError::Busy),
            None => page.prepare_hardware(writable)?,
        };
        Ok(Self {
            retention: Some(retention),
            phase: Phase::Prepared,
            external: None,
        })
    }
    pub(crate) fn prepare_external<H: Hardware<PageId>>(
        hardware: &H,
        page: &Arc<Page>,
        address: usize,
        writable: bool,
        change: Option<&Change>,
    ) -> Result<Self, Error> {
        let projection = Projection {
            id: BindingId::allocate()?,
            cell: page.id(),
            iova: IoAddress::page(address).ok_or(Error::InvalidRange)?,
            physical: page.physical_address(),
            size: page.size(),
            modifiable: writable,
        };
        if !address.is_multiple_of(projection.size)
            || address.checked_add(projection.size).is_none()
        {
            return Err(Error::InvalidRange);
        }
        hardware.validate(projection)?;
        let retention = match change {
            Some(change) if Arc::ptr_eq(change.page(), page) => change.external(writable)?,
            Some(_) => return Err(Error::InvalidRange),
            None => page.prepare_external(writable)?,
        };
        Ok(Self {
            retention: Some(retention),
            phase: Phase::Prepared,
            external: Some(projection),
        })
    }
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }
    /// Commit the obligation before the first possibly partial hardware write.
    pub(crate) fn publish(&mut self) {
        assert_eq!(self.phase, Phase::Prepared, "publish requires Prepared");
        self.retention
            .as_mut()
            .expect("prepared retention")
            .publish();
        self.phase = Phase::Publishing;
    }
    pub(crate) fn complete(&mut self) {
        assert!(
            matches!(self.phase, Phase::Publishing | Phase::Mapped),
            "complete requires published responsibility"
        );
        self.phase = Phase::Mapped;
    }
    fn begin_retirement(&mut self) {
        if matches!(self.phase, Phase::Publishing | Phase::Mapped) {
            self.retention = self.retention.take().map(Retention::retire);
        }
    }
    pub(crate) fn retire(mut self) -> Self {
        assert!(
            matches!(self.phase, Phase::Publishing | Phase::Mapped),
            "withdraw requires a published Publication"
        );
        self.begin_retirement();
        self.phase = Phase::Withdrawing;
        self
    }
    /// # Safety
    /// The exact CPU publication has been withdrawn and its covering fence completed.
    pub(crate) unsafe fn settle(mut self) {
        assert_eq!(
            self.phase,
            Phase::Withdrawing,
            "settle requires Withdrawing"
        );
        unsafe { self.retention.take().expect("retiring retention").settle() };
        self.phase = Phase::Settled;
    }
    pub(crate) fn submit<H: Hardware<PageId>>(&mut self, hardware: &mut H) -> Result<(), Error> {
        if self.phase != Phase::Prepared {
            return Err(Error::InvalidState);
        }
        let projection = self.external.ok_or(Error::InvalidState)?;
        self.publish();
        if let Err(error) = hardware.publish(projection) {
            self.begin_retirement();
            self.phase = Phase::Failed;
            return Err(error.into());
        }
        Ok(())
    }
    pub(crate) fn ready<H: Hardware<PageId>>(&mut self, hardware: &mut H) -> Result<bool, Error> {
        if !matches!(self.phase, Phase::Publishing | Phase::Mapped) {
            return Err(Error::InvalidState);
        }
        match hardware.ready(self.external.ok_or(Error::InvalidState)?) {
            Ok(true) => {
                self.complete();
                Ok(true)
            }
            Ok(false) => Ok(false),
            Err(error) => {
                self.begin_retirement();
                self.phase = Phase::Failed;
                Err(error.into())
            }
        }
    }
    pub(crate) fn revoke<H: Hardware<PageId>>(
        &mut self,
        hardware: &mut H,
    ) -> Result<Option<BatchId>, Error> {
        if let Phase::Waiting(batch) = self.phase {
            return Ok(Some(batch));
        }
        if self.phase == Phase::Prepared {
            self.retention.take();
            self.phase = Phase::Settled;
            return Ok(None);
        }
        if !matches!(
            self.phase,
            Phase::Publishing | Phase::Mapped | Phase::Failed
        ) {
            return Err(Error::InvalidState);
        }
        let projection = self.external.ok_or(Error::InvalidState)?;
        let batch = BatchId::allocate()?;
        self.begin_retirement();
        self.phase = Phase::Withdrawing;
        match hardware
            .revoke(projection)
            .and_then(|()| hardware.fence(projection, batch))
        {
            Ok(()) => {
                self.phase = Phase::Waiting(batch);
                Ok(Some(batch))
            }
            Err(error) => {
                self.phase = Phase::Failed;
                Err(error.into())
            }
        }
    }
    pub(crate) fn poll<H: Hardware<PageId>>(&mut self, hardware: &mut H) -> Result<bool, Error> {
        let Phase::Waiting(batch) = self.phase else {
            return Err(Error::InvalidState);
        };
        match hardware.observe(batch) {
            Ok(None) => Ok(false),
            Ok(Some(seen)) if seen == batch => {
                // SAFETY: installed backend covers this binding's exact fresh batch,
                // including platform ordering. No Unit can provide this completion.
                unsafe { self.retention.take().expect("retiring retention").settle() };
                self.phase = Phase::Settled;
                Ok(true)
            }
            Ok(Some(_)) => Err(Error::WrongBatch),
            Err(error) => {
                self.phase = Phase::Failed;
                Err(error.into())
            }
        }
    }
}
impl Deref for Publication {
    type Target = Retention;
    fn deref(&self) -> &Retention {
        self.retention
            .as_ref()
            .expect("settled publication has no Page")
    }
}
