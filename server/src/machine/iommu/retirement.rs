//! Kernel-lifetime storage, independent of Team/Task objects. Trusted device
//! binding installs adapters before publishing any Unit data.
use crate::basis::memory::nest::iommu::retirement::{DomainId, Owner, Registry};
use crate::basis::memory::nest::iommu::{Error, Hardware, IoSpace};
use crate::machine::sync::SpinLock;
use alloc::boxed::Box;

type Backend = Box<dyn Hardware + Send>;
static DOMAINS: SpinLock<Registry<Backend>> = SpinLock::new(Registry::new());

pub(crate) fn install(owner: &Owner, hardware: Backend) -> Result<DomainId, (Error, Backend)> {
    DOMAINS.lock().attach(owner, hardware)
}

pub(crate) fn with_space<T>(
    owner: &Owner,
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

pub(crate) fn completion(
    id: DomainId,
) -> Option<crate::basis::memory::nest::iommu::retirement::Completion> {
    DOMAINS.lock().completion(id)
}

pub(crate) fn fault(
    id: DomainId,
) -> Result<Option<crate::basis::memory::nest::iommu::riscv::Fault>, Error> {
    DOMAINS.lock().fault(id)
}

pub(crate) fn close(id: DomainId) -> Result<(), Error> {
    DOMAINS.lock().close(id)
}

/// Current retirement failure, retained until a successful retry.
pub(crate) fn error(id: DomainId) -> Result<Option<Error>, Error> {
    DOMAINS.lock().error(id)
}
