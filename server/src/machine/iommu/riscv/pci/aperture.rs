//! Trusted platform routes, not a device inventory or a Unit ownership ledger.
//! Ordinary BAR accesses use Pend-authorized CPU mappings. Only protected PCI
//! configuration and IOMMU admission remain here.
use super::super::controllers::CONTROLLERS;
use super::super::retirement;
use super::Endpoint;
use super::config;
use super::config::Ecam;
use crate::basis::cell::page::Owner as PageOwner;
use crate::basis::flat::FlatRange;
use crate::machine::iommu::Error;
use crate::machine::iommu::HardwareError;
use crate::machine::iommu::retirement::Completion;
use crate::machine::iommu::retirement::DomainId;
use crate::machine::iommu::retirement::Owner;
use crate::machine::iommu::riscv::adapter::Quiescence;
use crate::machine::sync::SpinLock;
use alloc::boxed::Box;
use alloc::vec::Vec;

pub(crate) struct Aperture {
    pub host: usize,
    pub rid: u16,
    pub did: u32,
    pub port: Endpoint<Ecam>,
    pub config: usize,
    pub source: FlatRange,
    pub used: bool,
}
pub(crate) static APERTURES: SpinLock<Vec<Aperture>> = SpinLock::new(Vec::new());
pub(crate) struct Binding {
    pub port: usize,
    pub did: u32,
    pub domain: DomainId,
    pub completion: Completion,
}
struct QemuCompletion {
    config: usize,
}
// SAFETY: only the explicit coherent QEMU EDU contract is admitted, without
// ATS/PRI/passthrough. Adapter calls after DDTP/queues Off; synchronous pci_dma
// callbacks precede this config completion and future transactions remain denied.
unsafe impl Quiescence for QemuCompletion {
    fn poll(&mut self) -> Result<bool, HardwareError> {
        config::order();
        let value = unsafe { (self.config as *const u32).read_volatile() };
        config::order();
        if value != 0x11e81234 {
            Err(HardwareError::Fault)
        } else {
            Ok(true)
        }
    }
}
pub(crate) fn bind(source: &FlatRange, owner: &Owner) -> Result<Binding, Error> {
    let mut ports = APERTURES.lock();
    let (port, entry) = ports
        .iter_mut()
        .enumerate()
        .find(|(_, p)| p.source.same_range(source))
        .ok_or(Error::InvalidRange)?;
    if entry.used {
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
    // Consume the platform route before binding; failures never allow reusing a
    // controller whose queues or address-directory effects may already exist.
    entry.used = true;
    let backend = unsafe {
        controllers.bind(
            entry.host,
            entry.rid,
            tables,
            QemuCompletion {
                config: entry.config,
            },
        )
    }
    .map_err(|_| Error::InvalidState)?;
    let backend = Box::try_new(backend).map_err(|_| Error::OutOfMemory)?;
    let domain = retirement::install(owner, backend).map_err(|(error, _)| error)?;
    let completion = retirement::completion(domain).expect("installed domain");
    Ok(Binding {
        port,
        did: entry.did,
        domain,
        completion,
    })
}
pub(crate) fn activate(port: usize) -> Result<(), Error> {
    let mut ports = APERTURES.lock();
    let entry = ports.get_mut(port).ok_or(Error::InvalidRange)?;
    // SAFETY: caller has verified the matching retained IoSpace is Active.
    unsafe { entry.port.activate() }.map_err(|_| Error::Hardware(HardwareError::Fault))
}
pub(crate) fn stop(port: usize) -> Result<(), Error> {
    APERTURES
        .lock()
        .get_mut(port)
        .ok_or(Error::InvalidRange)?
        .port
        .stop()
        .map_err(|_| Error::Hardware(HardwareError::Fault))
}
