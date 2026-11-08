//! Validate and install the exact PCI route supplied by the trusted boot image.

use crate::basis::flat::BorrowedRegion;
use crate::basis::flat::Flat;
use crate::basis::nest::Access;
use crate::basis::nest::Placement;
use crate::basis::nest::RegionKind;
use crate::basis::unit::team::kernel;
use crate::machine::board;
use crate::machine::iommu::riscv::pci::Ecam;
use crate::machine::iommu::riscv::pci::Endpoint;
use crate::machine::iommu::riscv::pci::aperture::{APERTURES, Aperture, QuiescenceStatus};
use crate::machine::iommu::riscv::topology::Topology;
use board::tree::{self, mmio, property, scalar};
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::{PAGE_SIZE, PhysAddr};

/// A platform-selected endpoint and its reserved BAR placement.
pub struct PciRoute {
    pub host: usize,
    pub requester: u16,
    pub identity: u32,
    pub bar: usize,
    pub quiescence: fn(&Ecam) -> QuiescenceStatus,
}

pub(super) fn mapped(base: usize, size: usize) -> Result<usize, PendFail> {
    let nest = &kernel().ok_or(PendFail::Denied)?.nest;
    let borrowed = unsafe { BorrowedRegion::new(PhysAddr::from_raw(base), size) }
        .map_err(|_| PendFail::Denied)?;
    let source = Flat::from_region(borrowed)
        .map_err(|_| PendFail::OutOfMemory)?
        .whole();
    let placement =
        Placement::lowest(RegionKind::Normal, size, 4096).map_err(|_| PendFail::Denied)?;
    let reserve = nest.reserve(placement).map_err(|_| PendFail::OutOfMemory)?;
    let access = Access::READ | Access::WRITE;
    let mapping = nest
        .attach(reserve, source, access, access)
        .map_err(|_| PendFail::Denied)?;
    Ok(mapping.initial_start().as_usize())
}

pub(in super::super) fn init(route: PciRoute) -> Result<(), PendFail> {
    let tree = board::device_tree().ok_or(PendFail::Denied)?;
    super::validate(&tree)?;
    let topology = Topology::discover(&tree).map_err(|_| PendFail::Denied)?;
    let did = topology
        .resolve(route.host, route.requester)
        .map_err(|_| PendFail::Denied)?
        .device();
    let (start, end) = bar_window(&tree, route.host)?;
    if crate::machine::iommu::riscv::interrupt::overlaps(start, end)
        || topology
            .configurations()
            .any(|(base, size)| start < base + size && base < end)
        || topology
            .controllers()
            .any(|base| start < base + PAGE_SIZE && base < end)
    {
        return Err(PendFail::Denied);
    }
    let mut apertures = APERTURES.lock();
    if !apertures.is_empty() {
        return Err(PendFail::Denied);
    }
    let config_offset = usize::from(route.requester) * PAGE_SIZE;
    let config_va = mapped(route.host + config_offset, PAGE_SIZE)?;
    let config = unsafe { Ecam::new(config_va) };
    let mut endpoint = Endpoint::discover(config).map_err(|_| PendFail::Denied)?;
    let size = endpoint.size();
    if endpoint.identity() != route.identity
        || route.bar < start
        || !route.bar.is_multiple_of(size)
        || route.bar.checked_add(size).is_none_or(|edge| edge > end)
    {
        return Err(PendFail::Denied);
    }
    apertures
        .try_reserve(1)
        .map_err(|_| PendFail::OutOfMemory)?;
    let borrowed = unsafe { BorrowedRegion::new(PhysAddr::from_raw(route.bar), size) }
        .map_err(|_| PendFail::Denied)?;
    let source = Flat::from_region(borrowed)
        .map_err(|_| PendFail::OutOfMemory)?
        .whole();
    // Only the validated BAR is exported. Config and IOMMU registers stay private.
    unsafe { endpoint.place(route.bar) }.map_err(|_| PendFail::Denied)?;
    endpoint.decode().map_err(|_| PendFail::Denied)?;
    apertures.push(Aperture {
        host: route.host,
        rid: route.requester,
        did,
        port: endpoint,
        config: config_va,
        quiescence: route.quiescence,
        source,
        used: false,
    });
    log::info!("iommu: {} trusted platform apertures", apertures.len());
    Ok(())
}

/// Identity-translated 32-bit MMIO window on a bus-zero ECAM host.
fn bar_window(tree: &fdt::Fdt, host: usize) -> Result<(usize, usize), PendFail> {
    let mut selected = None;
    for node in tree.all_nodes() {
        if property(node, "iommu-map")?.is_some() && mmio::single(tree, node)?.base == host {
            if selected.replace(node).is_some() {
                return Err(PendFail::Denied);
            }
        }
    }
    let node = selected.ok_or(PendFail::Denied)?;
    let bus = property(node, "bus-range")?.ok_or(PendFail::Denied)?;
    if bus.len() != 8 {
        return Err(PendFail::Denied);
    }
    let first_bus = &bus[..4];
    if first_bus != [0, 0, 0, 0] {
        return Err(PendFail::Denied);
    }
    let parent = tree::parent(tree, node).ok_or(PendFail::Denied)?;
    if scalar(node, "#address-cells")? != Some(3)
        || scalar(node, "#size-cells")? != Some(2)
        || scalar(parent, "#address-cells")? != Some(2)
    {
        return Err(PendFail::Denied);
    }
    let ranges = property(node, "ranges")?.ok_or(PendFail::Denied)?;
    if !ranges.len().is_multiple_of(28) {
        return Err(PendFail::Denied);
    }
    let mut window = None;
    for range in ranges.chunks_exact(28) {
        let word = |at: usize| u32::from_be_bytes(range[at * 4..at * 4 + 4].try_into().unwrap());
        if word(0) != 0x02000000 {
            continue;
        }
        let child = ((word(1) as u64) << 32) | word(2) as u64;
        let parent = ((word(3) as u64) << 32) | word(4) as u64;
        let size = ((word(5) as u64) << 32) | word(6) as u64;
        let end = parent.checked_add(size).ok_or(PendFail::Denied)?;
        if child != parent || parent == 0 || size == 0 || end > 1u64 << 32 || window.is_some() {
            return Err(PendFail::Denied);
        }
        window = Some((parent as usize, end as usize));
    }
    window.ok_or(PendFail::Denied)
}
