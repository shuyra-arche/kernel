//! Cross-axis join: private Pend borrow + stable Flat residency + Kernel IoSpace.
use super::{DEVICES, Entry, config, entry};
use crate::core::memory::{
    flat::allocator::frame,
    nest::iommu::{
        BindingId, Error as IoError, HardwareError, Phase as IoPhase,
        retirement::{Completion, DomainId, Owner},
        riscv::adapter::Quiescence,
    },
};
use crate::core::work::{
    device::{Phase, pci::Error},
    pend::ExternalBorrow,
};
use crate::machine::iommu::{controllers::CONTROLLERS, retirement};
use alloc::boxed::Box;

pub(crate) struct Dma {
    domain: DomainId,
    binding: Option<BindingId>,
    completion: Completion,
    borrow: Option<ExternalBorrow>,
}

struct QemuCompletion {
    config: usize,
}
// SAFETY: installed only under the explicit qemu-edu-dma deployment contract,
// for emulated EDU without ATS/PRI/host-memory caching. Adapter calls this only
// after DDTP/queues are Off. QEMU EDU's pci_dma callbacks complete synchronously;
// a PCI config completion orders prior callbacks and future DMA is denied by Off.
// This is not a device business-completion check and is never used on passthrough.
unsafe impl Quiescence for QemuCompletion {
    fn poll(&mut self) -> Result<bool, HardwareError> {
        config::order();
        let value = unsafe { (self.config as *const u32).read_volatile() };
        config::order();
        if value != 0x11e81234 {
            return Err(HardwareError::Fault);
        }
        Ok(true)
    }
}

fn io_error(error: IoError) -> Error {
    match error {
        IoError::OutOfMemory | IoError::Capacity => Error::Busy,
        IoError::InvalidRange | IoError::Overlap => Error::Invalid,
        IoError::Hardware(_) => Error::Hardware,
        _ => Error::Denied,
    }
}

pub(crate) fn publish(
    id: usize,
    owner: &Owner,
    borrow: ExternalBorrow,
    iova: usize,
    modifiable: bool,
) -> Result<(), Error> {
    let mut devices = DEVICES.lock();
    let entry = entry(&mut devices, id)?;
    if !entry.endpoint.owned_by(owner) {
        return Err(Error::Denied);
    }
    if entry.endpoint.phase() != Phase::Owned || entry.dma.is_some() {
        return Err(Error::Busy);
    }
    if !cfg!(feature = "qemu-edu-dma") || entry.endpoint.identity() != 0x11e81234 {
        return Err(Error::Unsupported);
    }
    if iova == 0 || !iova.is_multiple_of(4096) || iova >= 1usize << 38 {
        return Err(Error::Invalid);
    }
    let tables = [frame::cell(), frame::cell(), frame::cell(), frame::cell()];
    let [a, b, c, d] = tables;
    let tables = [
        a.map_err(|_| Error::Busy)?,
        b.map_err(|_| Error::Busy)?,
        c.map_err(|_| Error::Busy)?,
        d.map_err(|_| Error::Busy)?,
    ];
    let mut controllers = CONTROLLERS.lock();
    let controllers = controllers.as_mut().ok_or(Error::Unsupported)?;
    // Exclusive endpoint config/aperture, trusted route and explicit QEMU proof.
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
    .map_err(|_| Error::Unsupported)?;
    let backend = Box::try_new(backend).map_err(|_| Error::Busy)?;
    let domain = retirement::install(owner, backend).map_err(|(error, _)| io_error(error))?;
    let completion = retirement::completion(domain).expect("just installed record");
    // Keep the borrow record before any payload publication, including failures.
    entry.dma = Some(Dma {
        domain,
        binding: None,
        completion,
        borrow: Some(borrow),
    });
    let dma = entry.dma.as_mut().unwrap();
    let result = retirement::with_space(owner, domain, |space| {
        let borrowed = dma.borrow.as_mut().unwrap();
        // Pend excludes CPU/source aliases, Pages pins the exact Flat cell;
        // Controllers and Endpoint establish device ownership and routing.
        let binding = unsafe { space.register(borrowed.cell(), iova, modifiable) }?;
        dma.binding = Some(binding);
        borrowed.publish();
        space.publish(binding)
    });
    if let Err(error) = result {
        let _ = entry.endpoint.retire();
        let _ = retirement::with_space(owner, domain, |space| space.close());
        return Err(io_error(error));
    }
    Ok(())
}

pub(crate) fn status(id: usize, owner: &Owner) -> Result<usize, Error> {
    let mut devices = DEVICES.lock();
    let entry = entry(&mut devices, id)?;
    if !entry.endpoint.owned_by(owner) {
        return Err(Error::Denied);
    }
    if entry.queue_failed {
        return Ok(5);
    }
    if entry.endpoint.phase() == Phase::Owned
        && let Some(dma) = entry.dma.as_ref()
    {
        let binding = dma.binding.ok_or(Error::Hardware)?;
        let ready =
            retirement::with_space(owner, dma.domain, |space| match space.phase(binding)? {
                IoPhase::Publishing => space.ready(binding),
                IoPhase::Active => Ok(true),
                _ => Err(IoError::InvalidState),
            });
        let ready = match ready {
            Ok(ready) => ready,
            Err(error) => {
                entry.queue_failed = true;
                let _ = retirement::close(dma.domain);
                return Err(io_error(error));
            }
        };
        if ready {
            unsafe { entry.endpoint.activate(owner) }?;
        } else {
            return Ok(1);
        }
    }
    Ok(match entry.endpoint.phase() {
        Phase::Owned => 0,
        Phase::Active => 2,
        Phase::Retiring => 3,
        Phase::Closed => 4,
        Phase::Failed => 5,
        Phase::Available => return Err(Error::Denied),
    })
}

pub(crate) fn retire(id: usize, owner: &Owner) -> Result<(), Error> {
    let mut devices = DEVICES.lock();
    let entry = entry(&mut devices, id)?;
    if !entry.endpoint.owned_by(owner) {
        return Err(Error::Denied);
    }
    entry.endpoint.retire()?;
    if let Some(dma) = &entry.dma {
        retirement::with_space(owner, dma.domain, |space| space.close()).map_err(io_error)?;
    }
    Ok(())
}

/// Called under work commit + device lock. Completion is a Kernel-minted receipt
/// for this exact record, never a Unit acknowledgement or merely a CQ interrupt.
pub(super) fn settle(entry: &mut Entry) {
    if entry.endpoint.phase() == Phase::Retiring
        && entry
            .dma
            .as_ref()
            .is_none_or(|dma| dma.completion.settled())
    {
        if let Some(mut dma) = entry.dma.take() {
            if let Some(borrow) = dma.borrow.take() {
                unsafe { borrow.settle() };
            }
        }
        // Without a DMA record, bus mastering was never enabled by Kernel.
        unsafe { entry.endpoint.settle() }.expect("retiring endpoint");
    }
}

pub(crate) fn observe(id: usize) -> Result<usize, Error> {
    let mut devices = DEVICES.lock();
    let entry = entry(&mut devices, id)?;
    Ok(match entry.endpoint.phase() {
        Phase::Available => 0,
        Phase::Owned if entry.dma.is_some() => 1,
        Phase::Owned => 0,
        Phase::Active => 2,
        Phase::Retiring => 3,
        Phase::Closed => 4,
        Phase::Failed => 5,
    })
}

/// A bounded FQ drain. Retain the first fault as a level-triggered source;
/// consumers cannot acknowledge it into a hardware-settlement proof.
pub(super) fn poll_fault(entry: &mut Entry) {
    if entry.endpoint.phase() != Phase::Active {
        return;
    }
    let Some(dma) = &entry.dma else {
        return;
    };
    for _ in 0..4 {
        match retirement::fault(dma.domain) {
            Ok(Some(fault)) if fault.device == entry.did => {
                entry.fault.get_or_insert(fault);
            }
            Ok(Some(_)) => {
                entry.queue_failed = true;
            }
            Ok(None) => break,
            Err(_) => {
                entry.queue_failed = true;
                break;
            }
        }
    }
    if entry.queue_failed {
        let _ = retirement::close(dma.domain);
    }
}
