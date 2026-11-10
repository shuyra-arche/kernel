//! Admit a trusted source directly into its External Nest. Discovery and device
//! preparation stay in the trusted boot image; Unit calls never supply a requester.
use crate::basis::flat::{BorrowedRegion, Flat};
use crate::basis::nest::{Access, Nest, Placement, RegionKind};
use crate::basis::unit::team::kernel;
use crate::metal::generic::iommu::Quiescence;
use crate::metal::sync::SpinLock;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::{PAGE_SIZE, PhysAddr};
use kernel_api::boot::{ledger::Name, platform::Region};

pub(super) static REGIONS: SpinLock<Vec<Region>> = SpinLock::new(Vec::new());
// Boot staging only. After Ledger handoff, Nest/Pend own lifetime; there is no
// persistent basis-level source registry or second authorization tree.
pub(super) static NESTS: SpinLock<Vec<(Name, Arc<Nest>)>> = SpinLock::new(Vec::new());

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
    let mut staged = NESTS.lock();
    staged.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
    let nest = Nest::external_empty().map_err(|_| PendFail::OutOfMemory)?;
    // SAFETY: bind_external's trusted caller authenticated the actual source
    // and full isolation group. Controllers additionally enforces exclusivity.
    let hardware = unsafe {
        crate::metal::iommu::bind(route.controller, route.requester, route.proof)
    }
    .map_err(|_| PendFail::Denied)?;
    if let Err((_, hardware)) = nest.bind_external(hardware) {
        // Failure cannot discharge an already installed hardware obligation.
        drop(hardware);
        return Err(PendFail::Denied);
    }
    staged.push((
        Name::Nest {
            controller: route.controller as u64,
            requester: route.requester,
        },
        nest,
    ));
    Ok(())
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
