//! Trusted source admission. The backend owns the binding; the controller pool
//! regains its engine only after the exact Binding and platform proof settles.
use super::{adapter::Adapter, controllers::CONTROLLERS, mmio::Mmio};
use crate::basis::cell::page::{Owner as PageOwner, PageId};
use crate::basis::nest::PublicationError as Error;
use crate::metal::generic::iommu::{Hardware, Quiescence};
use crate::metal::iommu::{Backend, BatchId, Fault, HardwareError, Projection};
use alloc::boxed::Box;

struct Bound {
    adapter: Adapter<Mmio, Box<dyn Quiescence>>,
}
// SAFETY: forwarding retains the exclusive adapter. The controller is returned
// only after shutdown settles payloads and the independent Binding proof.
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

/// # Safety
/// Trusted platform admission established this immutable controller/requester
/// route and its complete isolation group. The proof covers all its requests;
/// bypass, ATS/PRI, noncoherent access and competing raw configuration are absent.
/// No Unit call may supply these arguments or the completion provider.
pub(crate) unsafe fn bind(
    controller: usize,
    requester: u32,
    proof: Box<dyn Quiescence>,
) -> Result<Backend, Error> {
    let [a, b, c, d] = core::array::from_fn::<_, 4, _>(|_| {
        crate::basis::cell::allocate(PageOwner::Kernel).map_err(|_| Error::OutOfMemory)
    });
    let tables = [a?, b?, c?, d?];
    // All allocation precedes consuming the exclusive controller slot.
    let mut storage =
        Box::try_new(core::mem::MaybeUninit::<Bound>::uninit()).map_err(|_| Error::OutOfMemory)?;
    let mut controllers = CONTROLLERS.lock();
    let adapter = unsafe {
        controllers
            .as_mut()
            .ok_or(Error::InvalidState)?
            .bind(controller, requester, tables, proof)
    }
    .map_err(|_| Error::InvalidState)?;
    storage.write(Bound { adapter });
    Ok(unsafe { storage.assume_init() })
}
