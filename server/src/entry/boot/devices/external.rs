//! Validate a trusted external resource and its immutable translation route.
//! Hardware discovery/configuration is performed by the caller's boot image.
use crate::basis::flat::{BorrowedRegion, Flat};
use crate::basis::port::Port;
use crate::metal::sync::SpinLock;
use alloc::vec::Vec;
use kernel_api::boot::platform::Region;
pub(super) static REGIONS: SpinLock<Vec<Region>> = SpinLock::new(Vec::new());
use crate::basis::nest::{Access, Placement, RegionKind};
use crate::basis::unit::team::kernel;
use crate::metal::generic::iommu::Quiescence;
use crate::metal::iommu::port::Requester;
use alloc::boxed::Box;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::{PAGE_SIZE, PhysAddr};

/// An exclusively owned resource and normalized controller/source identity.
/// Firmware and bus identities are resolved by the trusted integration owner.
pub struct ExternalRoute {
    pub controller: usize,
    pub requester: u32,
    pub proof: Box<dyn Quiescence>,
}

/// Trusted preparation may map only MMIO it exclusively owns for this boot.
/// Calling the function requires the physical region to satisfy that contract.
pub type MapMmio = unsafe fn(usize, usize) -> Result<usize, PendFail>;

pub(in super::super) fn mapped(base: usize, size: usize) -> Result<usize, PendFail> {
    if base == 0
        || size == 0
        || !base.is_multiple_of(PAGE_SIZE)
        || !size.is_multiple_of(PAGE_SIZE)
        || base.checked_add(size).is_none()
    {
        return Err(PendFail::Denied);
    }
    let nest = &kernel().ok_or(PendFail::Denied)?.nest;
    let borrowed = unsafe { BorrowedRegion::new(PhysAddr::from_raw(base), size) }
        .map_err(|_| PendFail::Denied)?;
    let source = Flat::from_region(borrowed)
        .map_err(|_| PendFail::OutOfMemory)?
        .whole();
    let placement =
        Placement::lowest(RegionKind::Normal, size, PAGE_SIZE).map_err(|_| PendFail::Denied)?;
    let reserve = nest.reserve(placement).map_err(|_| PendFail::OutOfMemory)?;
    let access = Access::READ | Access::WRITE;
    let mapping = nest
        .attach(reserve, source, access, access)
        .map_err(|_| PendFail::Denied)?;
    Ok(mapping.initial_start().as_usize())
}

pub(in super::super) fn init(route: ExternalRoute) -> Result<(), PendFail> {
    let description = super::admission::description()?;
    if !description
        .controllers
        .iter()
        .any(|r| r.base == route.controller)
        || route.requester >= 64
    {
        return Err(PendFail::Denied);
    }
    let port = Box::try_new(Requester {
        controller: route.controller,
        id: route.requester,
        proof: crate::metal::iommu::port::Provider::new(route.proof)
            .map_err(|_| PendFail::OutOfMemory)?,
    })
    .map_err(|_| PendFail::OutOfMemory)?;
    Port::register(route.controller, route.requester, port)
}

/// Grant raw register access independently of external source management.
/// This never registers a requester or changes an IOMMU context.
pub(in super::super) fn grant(region: Region) -> Result<(), PendFail> {
    let description = super::admission::description()?;
    description.admits(region)?;
    let mut regions = REGIONS.lock();
    let overlap =
        |old: &Region| old.base < region.base + region.size && region.base < old.base + old.size;
    if regions.iter().any(overlap) || description.authorities.iter().any(overlap) {
        return Err(PendFail::Denied);
    }
    regions.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
    regions.push(region);
    Ok(())
}
