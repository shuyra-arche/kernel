//! An execution-domain lease. ASID allocation, geometry and RFENCE stay here.
use super::{identity, mode};
use crate::basis::nest::NestError;
use crate::metal::generic::mmu::{
    Domain as Contract,
    fence::{Request, Scope, Shootdown},
};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use identity::Asid;

pub(crate) struct Domain {
    instance: usize,
    identifier: Asid,
    geometry: mode::Geo,
    published: AtomicBool,
    release: AtomicU8,
}
impl Contract for Domain {
    type Identifier = Asid;
    type Geometry = mode::Geo;
    type CreateError = NestError;
    fn create(instance: usize, kernel: bool) -> Result<Self, NestError> {
        let identifier = if kernel {
            Asid::kernel()
        } else {
            Asid::try_allocate().ok_or(NestError::OutOfMemory)?
        };
        Ok(Self {
            instance,
            identifier,
            geometry: mode::geometry(mode::mode()),
            published: AtomicBool::new(false),
            release: AtomicU8::new(if kernel { 2 } else { 0 }),
        })
    }
    fn identifier(&self) -> Asid {
        self.identifier
    }
    fn geometry(&self) -> mode::Geo {
        self.geometry
    }
    fn publish(&self) {
        self.published.store(true, Ordering::Release);
    }
    fn published(&self) -> bool {
        self.published.load(Ordering::Acquire)
    }
    fn released(&self) -> bool {
        self.release.load(Ordering::Acquire) == 2
    }
    fn release(&self) -> Result<(), NestError> {
        match self
            .release
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {}
            Err(2) => return Ok(()),
            Err(_) => return Err(NestError::Busy),
        }
        // No Nest/Address lock is held while SBI waits for remote CPUs.
        let result = identity::deallocate(self.identifier).map_err(|_| NestError::Shootdown);
        self.release
            .store(if result.is_ok() { 2 } else { 0 }, Ordering::Release);
        result
    }
}
// SAFETY: the unique instance and ASID are checked before the backend covers all
// active Hart leases. Failed/partial SBI fences produce no completion.
unsafe impl Shootdown for Domain {
    type Target = Request<Asid>;
    type Error = NestError;
    fn invalidate(&self, request: &Self::Target) -> Result<(), NestError> {
        if request.domain != self.instance
            || request.identifier != self.identifier
            || self.released() && !self.identifier.is_kernel()
        {
            return Err(NestError::Shootdown);
        }
        let asid = match request.scope {
            Scope::AddressSpace => self.identifier,
            Scope::Global => Asid::kernel(),
        };
        identity::shootdown(asid).map_err(|_| NestError::Shootdown)
    }
}
impl Drop for Domain {
    fn drop(&mut self) {
        if !self.published() && !self.identifier.is_kernel() {
            Asid::release_unpublished(self.identifier);
        }
        // A published but unfinished domain deliberately keeps its ASID reserved.
    }
}
