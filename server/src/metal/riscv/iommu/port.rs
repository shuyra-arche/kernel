//! Reusable requester access. Reuse requires the old backend's exact Binding
//! proof; immutable platform facts and raw-device ownership remain trusted.
use super::{adapter::Adapter, controllers::CONTROLLERS, mmio::Mmio};
use crate::basis::cell::page::{Owner as PageOwner, PageId};
use crate::basis::nest::PublicationError as Error;
use crate::metal::generic::iommu::{Hardware, Port, Quiescence, Retirement};
use crate::metal::iommu::{Backend, BatchId, Fault, HardwareError, Projection};
use crate::metal::sync::SpinLock;
use alloc::{boxed::Box, sync::Arc};

#[derive(Clone)]
pub(crate) struct Provider(Arc<SpinLock<Box<dyn Quiescence>>>);
impl Provider {
    pub(crate) fn new(proof: Box<dyn Quiescence>) -> Result<Self, Error> {
        Ok(Self(
            Arc::try_new(SpinLock::new(proof)).map_err(|_| Error::OutOfMemory)?,
        ))
    }
}
// SAFETY: the shared provider is serialized; tokens retain exact scope/attempt.
unsafe impl Quiescence for Provider {
    fn prove(&mut self, ticket: Retirement) -> Result<Option<Retirement>, HardwareError> {
        self.0.lock().prove(ticket)
    }
}
pub(crate) struct Requester {
    pub(crate) controller: usize,
    pub(crate) id: u32,
    pub(crate) proof: Provider,
}
struct Bound {
    adapter: Adapter<Mmio, Provider>,
}
// SAFETY: forwarding retains the exclusive adapter. The controller is returned
// only after shutdown has completed every payload and the independent Binding proof.
unsafe impl Hardware<PageId> for Bound {
    fn capacity(&self) -> usize {
        self.adapter.capacity()
    }
    fn validate(&self, p: Projection) -> Result<(), HardwareError> {
        self.adapter.validate(p)
    }
    fn publish(&mut self, p: Projection) -> Result<(), HardwareError> {
        self.adapter.publish(p)
    }
    fn ready(&mut self, p: Projection) -> Result<bool, HardwareError> {
        self.adapter.ready(p)
    }
    fn revoke(&mut self, p: Projection) -> Result<(), HardwareError> {
        self.adapter.revoke(p)
    }
    fn fence(&mut self, p: Projection, b: BatchId) -> Result<(), HardwareError> {
        self.adapter.fence(p, b)
    }
    fn observe(&mut self, b: BatchId) -> Result<Option<BatchId>, HardwareError> {
        self.adapter.observe(b)
    }
    fn fault(&mut self) -> Result<Option<Fault>, HardwareError> {
        self.adapter.fault()
    }
    fn shutdown(&mut self) -> Result<bool, HardwareError> {
        if !self.adapter.shutdown()? {
            return Ok(false);
        }
        if let Some(engine) = self.adapter.take_engine() {
            let mut controllers = CONTROLLERS.lock();
            controllers
                .as_mut()
                .ok_or(HardwareError::Fault)?
                .restore(engine)
                .map_err(|_| HardwareError::Fault)?;
        }
        Ok(true)
    }
}
// SAFETY: boot validated the immutable route; Controllers enforces exclusivity.
unsafe impl Port<PageId> for Requester {
    type Error = Error;
    fn bind(&mut self) -> Result<Backend, Error> {
        let [a, b, c, d] = core::array::from_fn::<_, 4, _>(|_| {
            crate::basis::cell::allocate(PageOwner::Kernel).map_err(|_| Error::OutOfMemory)
        });
        let tables = [a?, b?, c?, d?];
        // Allocate the dynamic backend before consuming the controller.
        let mut storage = Box::try_new(core::mem::MaybeUninit::<Bound>::uninit())
            .map_err(|_| Error::OutOfMemory)?;
        let mut controllers = CONTROLLERS.lock();
        let adapter = unsafe {
            controllers.as_mut().ok_or(Error::InvalidState)?.bind(
                self.controller,
                self.id,
                tables,
                self.proof.clone(),
            )
        }
        .map_err(|_| Error::InvalidState)?;
        storage.write(Bound { adapter });
        Ok(unsafe { storage.assume_init() })
    }
}
