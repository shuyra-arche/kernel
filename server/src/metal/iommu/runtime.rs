//! Kernel-lifetime storage, independent of Team/Task objects. Trusted device
//! binding installs adapters before publishing any Unit data.
use crate::basis::cell::projection::Admission;
use crate::metal::iommu::Error;
use crate::metal::iommu::Hardware;
use crate::metal::iommu::IoSpace;
use crate::metal::iommu::retirement::DomainId;
use crate::metal::iommu::retirement::Registry;
use crate::metal::sync::SpinLock;
use alloc::boxed::Box;

pub(crate) type Backend = Box<dyn Hardware + Send>;
static DOMAINS: SpinLock<Registry<Backend>> = SpinLock::new(Registry::new());

pub(crate) fn install(owner: &Admission, hardware: Backend) -> Result<DomainId, (Error, Backend)> {
    DOMAINS.lock().attach(owner, hardware)
}

pub(crate) fn with_space<T>(
    owner: &Admission,
    id: DomainId,
    action: impl FnOnce(&mut IoSpace<Backend>) -> Result<T, Error>,
) -> Result<T, Error> {
    DOMAINS.lock().with_space(owner, id, action)
}

/// Timer fallback, no waiting for MMIO/queues and no lock spinning in the trap.
/// Queue interrupts and idle fallback also call this bounded progress path.
pub(crate) fn progress() -> bool {
    if let Some(mut domains) = DOMAINS.try_lock() {
        domains.step(4);
        domains.pending() != 0
    } else {
        true
    }
}

pub(crate) fn completion(id: DomainId) -> Option<crate::metal::iommu::retirement::Completion> {
    DOMAINS.lock().completion(id)
}

pub(crate) fn fault(id: DomainId) -> Result<Option<crate::metal::iommu::Fault>, Error> {
    DOMAINS.lock().fault(id)
}

pub(crate) fn close(id: DomainId) -> Result<(), Error> {
    DOMAINS.lock().close(id)
}

/// Current retirement failure, retained until a successful retry.
pub(crate) fn error(id: DomainId) -> Result<Option<Error>, Error> {
    DOMAINS.lock().error(id)
}
