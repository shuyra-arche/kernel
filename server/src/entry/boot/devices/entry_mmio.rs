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

enum Layout {
    Open(Vec<Mapping>),
    Sealed,
}
struct Mapping {
    physical: Region,
    address: VirtAddr,
}
static STAGED: SpinLock<Layout> = SpinLock::new(Layout::Open(Vec::new()));

pub(in crate::entry::boot) fn stage(region: Region, address: VirtAddr) -> Result<(), PendFail> {
    super::admission::description()?.admits(region)?;
    let end = address
        .get()
        .checked_add(region.size)
        .ok_or(PendFail::Denied)?;
    Placement::exact(RegionKind::Normal, address, region.size).map_err(|_| PendFail::Denied)?;
    let mut staged = STAGED.lock();
    let Layout::Open(mappings) = &mut *staged else {
        return Err(PendFail::Denied);
    };
    if mappings.iter().any(|old| {
        old.physical.base < region.base + region.size
            && region.base < old.physical.base + old.physical.size
            || old.address.get() < end && address.get() < old.address.get() + old.physical.size
    }) {
        return Err(PendFail::Denied);
    }
    mappings.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
    mappings.push(Mapping {
        physical: region,
        address,
    });
    Ok(())
}

pub(in crate::entry::boot) fn install(nest: &Arc<Nest>) -> Result<(), NestError> {
    let mappings = {
        let mut staged = STAGED.lock();
        match core::mem::replace(&mut *staged, Layout::Sealed) {
            Layout::Open(mappings) => mappings,
            Layout::Sealed => return Err(NestError::Busy),
        }
    };
    for Mapping {
        physical: region,
        address,
    } in mappings
    {
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
