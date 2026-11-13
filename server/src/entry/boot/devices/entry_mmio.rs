//! Explicit static device mappings for the initial image. These are boot layout,
//! not RAM Publications or dynamically revocable Share grants.
use crate::basis::{
    flat::{BorrowedRegion, Flat},
    nest::{Access, Nest, NestError, Placement, RegionKind},
};
use crate::metal::sync::SpinLock;
use alloc::{sync::Arc, vec::Vec};
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PhysAddr, VirtAddr},
    boot::platform::Region,
};

static STAGED: SpinLock<(bool, Vec<(Region, VirtAddr)>)> = SpinLock::new((false, Vec::new()));

pub(in crate::entry::boot) fn stage(region: Region, address: VirtAddr) -> Result<(), PendFail> {
    super::admission::description()?.admits(region)?;
    let end = address
        .get()
        .checked_add(region.size)
        .ok_or(PendFail::Denied)?;
    Placement::exact(RegionKind::Normal, address, region.size).map_err(|_| PendFail::Denied)?;
    let mut staged = STAGED.lock();
    if staged.0
        || staged.1.iter().any(|(old, at)| {
            old.base < region.base + region.size && region.base < old.base + old.size
                || at.get() < end && address.get() < at.get() + old.size
        })
    {
        return Err(PendFail::Denied);
    }
    staged.1.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
    staged.1.push((region, address));
    Ok(())
}

pub(in crate::entry::boot) fn install(nest: &Arc<Nest>) -> Result<(), NestError> {
    let mappings = {
        let mut staged = STAGED.lock();
        staged.0 = true;
        core::mem::take(&mut staged.1)
    };
    for (region, address) in mappings {
        // SAFETY: the trusted boot caller retains exclusive MMIO ownership for
        // this Team's lifetime. Admission excludes RAM and protected controls.
        let source = Flat::from_region(unsafe {
            BorrowedRegion::new(PhysAddr::from_raw(region.base), region.size)?
        })?
        .whole();
        let reservation =
            nest.reserve(Placement::exact(RegionKind::Normal, address, region.size)?)?;
        let access = Access::READ | Access::WRITE;
        nest.attach(reservation, source, access, access)?;
    }
    Ok(())
}
