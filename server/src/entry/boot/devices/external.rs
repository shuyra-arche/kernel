//! Validate a trusted external resource and its immutable translation route.
//! Hardware discovery/configuration is performed by the caller's boot image.
use crate::basis::flat::{BorrowedRegion, Flat};
use crate::basis::nest::authority::aperture::{APERTURES, Aperture};
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
    pub base: usize,
    pub size: usize,
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
    let region = kernel_api::boot::platform::Region::new(route.base, route.size);
    description.admits(region)?;
    if !description
        .controllers
        .iter()
        .any(|r| r.base == route.controller)
        || route.requester >= 64
    {
        return Err(PendFail::Denied);
    }
    let did = route.requester;
    let mut apertures = APERTURES.lock();
    if !apertures.is_empty() {
        return Err(PendFail::Denied);
    }
    apertures
        .try_reserve(1)
        .map_err(|_| PendFail::OutOfMemory)?;
    let borrowed = unsafe { BorrowedRegion::new(PhysAddr::from_raw(route.base), route.size) }
        .map_err(|_| PendFail::Denied)?;
    let source = Flat::from_region(borrowed)
        .map_err(|_| PendFail::OutOfMemory)?
        .whole();
    let port = Box::try_new(Requester {
        controller: route.controller,
        id: route.requester,
        proof: Some(route.proof),
    })
    .map_err(|_| PendFail::OutOfMemory)?;
    apertures.push(Aperture {
        base: route.base,
        did,
        port,
        source,
    });
    log::info!("iommu: {} trusted external apertures", apertures.len());
    Ok(())
}
