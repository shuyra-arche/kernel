//! One-shot requester binding. Kernel controls translation reachability;
//! endpoint configuration and platform ordering are supplied by trusted boot.
use alloc::boxed::Box;

use super::controllers::CONTROLLERS;
use crate::basis::cell::page::{Owner as PageOwner, PageId};
use crate::metal::generic::iommu::{Port, Quiescence};
use crate::metal::iommu::{Backend, Error};

pub(crate) struct Requester {
    pub(crate) host: usize,
    pub(crate) id: u16,
    pub(crate) proof: Option<Box<dyn Quiescence>>,
}

// SAFETY: boot validates the immutable firmware route and holds its resource
// authority. The platform implements the proof contract for the entire controller.
unsafe impl Port<PageId> for Requester {
    type Error = Error;

    fn bind(&mut self) -> Result<Backend, Error> {
        if self.proof.is_none() {
            return Err(Error::Closed);
        }
        let [a, b, c, d] = [
            crate::basis::cell::allocate(PageOwner::Kernel),
            crate::basis::cell::allocate(PageOwner::Kernel),
            crate::basis::cell::allocate(PageOwner::Kernel),
            crate::basis::cell::allocate(PageOwner::Kernel),
        ];
        let tables = [a, b, c, d].map(|p| p.map_err(|_| Error::OutOfMemory));
        let [a, b, c, d] = tables;
        let tables = [a?, b?, c?, d?];
        let mut controllers = CONTROLLERS.lock();
        let controllers = controllers.as_mut().ok_or(Error::InvalidState)?;
        // Consume before any controller effect; failed binding cannot reuse proof.
        let proof = self.proof.take().ok_or(Error::Closed)?;
        let backend = unsafe { controllers.bind(self.host, self.id, tables, proof) }
            .map_err(|_| Error::InvalidState)?;
        Ok(Box::try_new(backend).map_err(|_| Error::OutOfMemory)?)
    }
}
