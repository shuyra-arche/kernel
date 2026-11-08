//! Validate a trusted external resource and its immutable translation route.
//! Hardware discovery/configuration is performed by the caller's boot image.
use crate::basis::flat::{BorrowedRegion, Flat};
use crate::basis::nest::{Access, Placement, RegionKind};
use crate::basis::unit::team::kernel;
use crate::metal::board;
use crate::metal::generic::iommu::Quiescence;
use crate::metal::iommu::aperture::{APERTURES, Aperture};
use crate::metal::iommu::riscv::port::Requester;
use crate::metal::iommu::riscv::topology::Topology;
use alloc::boxed::Box;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::{PAGE_SIZE, PhysAddr};

/// An exclusively owned, platform-configured resource window. Host/requester
/// are resolved through validated firmware topology, not used as a raw DID.
pub struct ExternalRoute {
    pub host: usize,
    pub requester: u16,
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
    let tree = board::device_tree().ok_or(PendFail::Denied)?;
    super::validate(&tree)?;
    let topology = Topology::discover(&tree).map_err(|_| PendFail::Denied)?;
    let did = topology
        .resolve(route.host, route.requester)
        .map_err(|_| PendFail::Denied)?
        .device();
    let region = board::tree::mmio::Region::new(route.base, route.size)?;
    let end = route.base.checked_add(route.size).ok_or(PendFail::Denied)?;
    let ram = board::info().memory.envelope();
    super::plan::Plan::discover(&tree, ram.base, ram.size)?.external(&tree, region)?;
    if !route.base.is_multiple_of(PAGE_SIZE)
        || !route.size.is_multiple_of(PAGE_SIZE)
        || region.overlaps(board::tree::mmio::Region::new(ram.base, ram.size)?)
        || crate::metal::interrupt::external().is_some_and(|irq| irq.overlaps(route.base, end))
        || topology
            .configurations()
            .any(|(base, size)| route.base < base + size && base < end)
        || topology
            .controllers()
            .any(|base| route.base < base + PAGE_SIZE && base < end)
    {
        return Err(PendFail::Denied);
    }
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
        host: route.host,
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
