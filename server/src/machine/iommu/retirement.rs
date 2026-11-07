//! Kernel-lifetime storage, independent of Team/Task objects. Trusted device
//! binding will install adapters here; normal boot still exposes no DMA ABI.
use crate::core::memory::nest::iommu::retirement::{DomainId, Owner, Registry};
use crate::core::memory::nest::iommu::{Error, Hardware, IoSpace};
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
/// Queue interrupts and Sign event routing remain separate follow-up work.
pub(crate) fn progress() {
    if let Some(mut domains) = DOMAINS.try_lock() {
        domains.step(4);
    }
}
