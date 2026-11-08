//! PCI admission and quiescence for the RV64 I/O backend.
use alloc::boxed::Box;

use super::adapter::Quiescence;
use super::controllers::CONTROLLERS;
use crate::basis::cell::page::Owner as PageOwner;
use crate::metal::iommu::{Error, HardwareError, aperture::Port, runtime};
use crate::metal::pci::{Ecam, Endpoint};

pub(crate) struct PciPort {
    pub(crate) host: usize,
    pub(crate) rid: u16,
    pub(crate) endpoint: Endpoint<Ecam>,
    pub(crate) config: usize,
    pub(crate) quiescence: fn(&Ecam) -> QuiescenceStatus,
    pub(crate) used: bool,
}

/// Outcome of the trusted platform check after IOMMU retirement.
pub enum QuiescenceStatus {
    Pending,
    Settled,
    Failed,
}

struct PlatformCompletion {
    config: Ecam,
    poll: fn(&Ecam) -> QuiescenceStatus,
}

// SAFETY: boot requires the platform to supply a bounded proof for this route.
unsafe impl Quiescence for PlatformCompletion {
    fn poll(&mut self) -> Result<bool, HardwareError> {
        match (self.poll)(&self.config) {
            QuiescenceStatus::Pending => Ok(false),
            QuiescenceStatus::Settled => Ok(true),
            QuiescenceStatus::Failed => Err(HardwareError::Fault),
        }
    }
}
// SAFETY: boot installs this only for the exact validated requester and BAR.
// Controllers::bind consumes the trusted engine and keeps the route one-shot.
unsafe impl Port for PciPort {
    fn bind(&mut self) -> Result<runtime::Backend, Error> {
        if self.used {
            return Err(Error::Closed);
        }
        let [a, b, c, d] = [
            crate::basis::cell::allocate(PageOwner::Kernel),
            crate::basis::cell::allocate(PageOwner::Kernel),
            crate::basis::cell::allocate(PageOwner::Kernel),
            crate::basis::cell::allocate(PageOwner::Kernel),
        ];
        let tables = [
            a.map_err(|_| Error::OutOfMemory)?,
            b.map_err(|_| Error::OutOfMemory)?,
            c.map_err(|_| Error::OutOfMemory)?,
            d.map_err(|_| Error::OutOfMemory)?,
        ];
        let mut controllers = CONTROLLERS.lock();
        let controllers = controllers.as_mut().ok_or(Error::InvalidState)?;
        // Once controller effects are possible, this port is consumed even on error.
        self.used = true;
        let backend = unsafe {
            controllers.bind(
                self.host,
                self.rid,
                tables,
                PlatformCompletion {
                    config: Ecam::new(self.config),
                    poll: self.quiescence,
                },
            )
        }
        .map_err(|_| Error::InvalidState)?;
        Ok(Box::try_new(backend).map_err(|_| Error::OutOfMemory)?)
    }

    unsafe fn activate(&mut self) -> Result<(), Error> {
        unsafe { self.endpoint.activate() }.map_err(|_| Error::Hardware(HardwareError::Fault))
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.endpoint
            .stop()
            .map_err(|_| Error::Hardware(HardwareError::Fault))
    }
}
