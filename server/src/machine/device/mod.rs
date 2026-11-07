//! Kernel-owned PCI configuration and per-device BAR access. No raw ECAM or
//! aperture is delegated. The lock covers authority checks and hardware effects.
mod config;
mod dma;
use crate::core::memory::nest::iommu::retirement::Owner;
use crate::core::work::device::{
    Phase,
    pci::{Endpoint, Error},
};
use crate::machine::sync::SpinLock;
use alloc::vec::Vec;
pub(crate) use config::Ecam;
use core::sync::atomic::{AtomicUsize, Ordering};
pub(crate) use dma::{observe, publish, retire, status};
static OUTSTANDING: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Entry {
    pub(crate) host: usize,
    pub(crate) rid: u16,
    pub(crate) did: u32,
    pub(crate) endpoint: Endpoint<Ecam>,
    pub(crate) bar: usize,
    pub(crate) mapped: usize,
    pub(crate) config: usize,
    pub(crate) events: alloc::sync::Arc<crate::core::work::unit::life::Life>,
    pub(crate) fault: Option<crate::core::memory::nest::iommu::riscv::Fault>,
    pub(crate) queue_failed: bool,
    pub(crate) dma: Option<dma::Dma>,
}
pub(crate) static DEVICES: SpinLock<Vec<Entry>> = SpinLock::new(Vec::new());

pub(crate) fn scan(after: usize) -> Option<(usize, usize)> {
    DEVICES
        .lock()
        .iter()
        .enumerate()
        .skip(after)
        .find(|(_, entry)| entry.endpoint.phase() == Phase::Available)
        .map(|(index, entry)| (index + 1, entry.endpoint.identity() as usize))
}

fn entry(devices: &mut [Entry], id: usize) -> Result<&mut Entry, Error> {
    devices
        .get_mut(id.checked_sub(1).ok_or(Error::Denied)?)
        .ok_or(Error::Denied)
}

pub(crate) fn claim(id: usize, owner: &Owner) -> Result<(), Error> {
    let mut devices = DEVICES.lock();
    let endpoint = &mut entry(&mut devices, id)?.endpoint;
    let before = endpoint.phase();
    let result = endpoint.claim(owner);
    if before == Phase::Available && endpoint.phase() != before {
        OUTSTANDING.fetch_add(1, Ordering::Release);
    }
    result
}

pub(crate) fn inspect(id: usize, owner: &Owner, offset: usize) -> Result<usize, Error> {
    entry(&mut DEVICES.lock(), id)?
        .endpoint
        .inspect(owner, offset)
        .map(|v| v as usize)
}

pub(crate) fn read(id: usize, owner: &Owner, offset: usize, width: usize) -> Result<u64, Error> {
    let mut devices = DEVICES.lock();
    let entry = entry(&mut devices, id)?;
    let physical = entry.endpoint.access(owner, offset, width)?;
    let at = entry.mapped + (physical - entry.bar);
    config::order();
    let value = unsafe {
        match width {
            1 => (at as *const u8).read_volatile() as u64,
            2 => u16::from_le((at as *const u16).read_volatile()) as u64,
            4 => u32::from_le((at as *const u32).read_volatile()) as u64,
            8 => u64::from_le((at as *const u64).read_volatile()),
            _ => unreachable!(),
        }
    };
    config::order();
    Ok(value)
}

pub(crate) fn write(
    id: usize,
    owner: &Owner,
    offset: usize,
    width: usize,
    value: u64,
) -> Result<(), Error> {
    let mut devices = DEVICES.lock();
    let entry = entry(&mut devices, id)?;
    let physical = entry.endpoint.access(owner, offset, width)?;
    if width < 8 && value >> (width * 8) != 0 {
        return Err(Error::Invalid);
    }
    let at = entry.mapped + (physical - entry.bar);
    config::order();
    unsafe {
        match width {
            1 => (at as *mut u8).write_volatile(value as u8),
            2 => (at as *mut u16).write_volatile((value as u16).to_le()),
            4 => (at as *mut u32).write_volatile((value as u32).to_le()),
            8 => (at as *mut u64).write_volatile(value.to_le()),
            _ => unreachable!(),
        }
    };
    config::order();
    Ok(())
}

/// Bounded timer fallback. Admission closes immediately with Owner; this step
/// disables the PCI command register independently of Driver cooperation.
/// Retiring is retained until its DMA completion receipt has settled.
pub(crate) fn progress() {
    static CURSOR: AtomicUsize = AtomicUsize::new(0);
    let _commit = crate::core::work::commit();
    for _ in 0..4 {
        let changed = {
            let Some(mut devices) = DEVICES.try_lock() else {
                return;
            };
            if devices.is_empty() {
                return;
            }
            let at = CURSOR.fetch_add(1, Ordering::Relaxed) % devices.len();
            let entry = &mut devices[at];
            let before = entry.endpoint.phase();
            let had_fault = entry.fault.is_some() || entry.queue_failed;
            dma::poll_fault(entry);
            if (entry.endpoint.owner_closed() || entry.queue_failed)
                && matches!(before, Phase::Owned | Phase::Active | Phase::Failed)
            {
                let _ = entry.endpoint.retire();
            }
            dma::settle(entry);
            if before != Phase::Closed && entry.endpoint.phase() == Phase::Closed {
                OUTSTANDING.fetch_sub(1, Ordering::Release);
            }
            (before != entry.endpoint.phase()
                || had_fault != (entry.fault.is_some() || entry.queue_failed))
                .then_some(at + 1)
        };
        if let Some(id) = changed {
            crate::core::work::sign::signal(crate::core::work::sign::WakeKey::Device { id });
        }
    }
}

/// A persistent Kernel source, not the requesting Task's life. Polling rechecks
/// current authority, so routing never turns a stale subscription into access.
pub(crate) fn event(
    actor: &crate::core::work::unit::task::Task,
    id: usize,
    fault: bool,
) -> Result<
    (
        alloc::sync::Weak<crate::core::work::unit::life::Life>,
        usize,
    ),
    Error,
> {
    let _commit = crate::core::work::commit();
    let manager = crate::core::work::pend::allows_locked(
        actor,
        &crate::core::work::mail::Mail::Nole(crate::entry::call::resources::get().device.clone()),
        crate::core::work::pend::Need::Fetch,
    );
    let mut devices = DEVICES.lock();
    let entry = entry(&mut devices, id)?;
    if !manager && !entry.endpoint.owned_by(&actor.ident.team.space.dma) {
        return Err(Error::Denied);
    }
    let phase = if entry.queue_failed {
        5
    } else if fault {
        if entry.fault.is_some() { 4 } else { 0 }
    } else {
        match entry.endpoint.phase() {
            Phase::Available | Phase::Owned => 0,
            Phase::Active => 2,
            Phase::Retiring => 3,
            Phase::Closed => 4,
            Phase::Failed => 5,
        }
    };
    Ok((alloc::sync::Arc::downgrade(&entry.events), phase))
}

/// The idle loop must service this too: WFI wakeups need not enter a trap.
pub(crate) fn maintain() -> bool {
    let pending = crate::machine::iommu::retirement::progress();
    progress();
    crate::machine::iommu::interrupt::rearm();
    pending || OUTSTANDING.load(Ordering::Acquire) != 0
}

pub(crate) fn fault(
    actor: &crate::core::work::unit::task::Task,
    id: usize,
) -> Result<(usize, usize), Error> {
    event(actor, id, true)?;
    let mut devices = DEVICES.lock();
    let entry = entry(&mut devices, id)?;
    if entry.queue_failed {
        return Err(Error::Hardware);
    }
    let fault = entry.fault.ok_or(Error::Busy)?;
    Ok((fault.cause as usize, fault.address as usize))
}
