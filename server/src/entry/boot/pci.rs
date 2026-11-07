//! Restricted boot census: direct bus-0 conventional PCI functions and one
//! identity-translated non-prefetchable 32-bit aperture per protected host.
//! The consumer never receives either the configuration or aperture mapping.
use crate::core::memory::nest::iommu::riscv::topology::Topology;
use crate::core::memory::{
    address::PhysAddr,
    flat::{BorrowedRegion, Flat},
    nest::{Access, Placement, RegionId},
};
use crate::core::work::{
    device::pci::{Endpoint, Error},
    unit::team::kernel,
};
use crate::machine::{
    board,
    device::{DEVICES, Ecam, Entry},
};
use alloc::vec::Vec;
use kernel_api::abi::call::pend::PendFail;

pub(super) fn mapped(base: usize, size: usize) -> Result<usize, PendFail> {
    let nest = &kernel().ok_or(PendFail::Denied)?.space.nest;
    let borrowed = unsafe { BorrowedRegion::new(PhysAddr::from_raw(base), size) }
        .map_err(|_| PendFail::Denied)?;
    let source = Flat::from_region(borrowed)
        .map_err(|_| PendFail::OoM)?
        .whole();
    let placement =
        Placement::lowest(RegionId::Normal, size, 4096).map_err(|_| PendFail::Denied)?;
    let reserve = nest.reserve(placement).map_err(|_| PendFail::OoM)?;
    let access = Access::READ | Access::WRITE;
    let mapping = nest
        .attach(reserve, source, access, access)
        .map_err(|_| PendFail::Denied)?;
    Ok(mapping.initial_start().as_usize())
}

pub(super) fn init() -> Result<(), PendFail> {
    let tree = board::device_tree().ok_or(PendFail::Denied)?;
    if !tree.all_nodes().any(|n| n.property("iommu-map").is_some()) {
        return Ok(());
    }
    super::devices::validate(&tree)?;
    let topology = Topology::discover(&tree).map_err(|_| PendFail::Denied)?;
    let mut devices = DEVICES.lock();
    if !devices.is_empty() {
        return Err(PendFail::Denied);
    }
    let mut allocated: Vec<(usize, usize)> = Vec::new();
    for (host, _) in topology.configurations() {
        let node = tree
            .all_nodes()
            .find(|n| {
                n.reg().is_some_and(|mut r| {
                    r.next().is_some_and(|r| r.starting_address.addr() == host)
                })
            })
            .ok_or(PendFail::Denied)?;
        let first_bus = node
            .property("bus-range")
            .and_then(|p| p.value.get(..4))
            .ok_or(PendFail::Denied)?;
        if first_bus != [0, 0, 0, 0] {
            return Err(PendFail::Denied);
        }
        let cells = |n: fdt::node::FdtNode, key| n.property(key).and_then(|p| p.as_usize());
        let parent = tree
            .all_nodes()
            .find(|p| p.children().any(|c| core::ptr::eq(c.name, node.name)))
            .ok_or(PendFail::Denied)?;
        if cells(node, "#address-cells") != Some(3)
            || cells(node, "#size-cells") != Some(2)
            || cells(parent, "#address-cells") != Some(2)
        {
            return Err(PendFail::Denied);
        }
        let ranges = node.property("ranges").ok_or(PendFail::Denied)?.value;
        if !ranges.len().is_multiple_of(28) {
            return Err(PendFail::Denied);
        }
        let mut aperture = None;
        for range in ranges.chunks_exact(28) {
            let word =
                |at: usize| u32::from_be_bytes(range[at * 4..at * 4 + 4].try_into().unwrap());
            if word(0) != 0x02000000 {
                continue;
            }
            let child = ((word(1) as u64) << 32) | word(2) as u64;
            let parent = ((word(3) as u64) << 32) | word(4) as u64;
            let size = ((word(5) as u64) << 32) | word(6) as u64;
            let end = parent.checked_add(size).ok_or(PendFail::Denied)?;
            if child != parent || parent == 0 || size == 0 || end > 1u64 << 32 || aperture.is_some()
            {
                return Err(PendFail::Denied);
            }
            aperture = Some((parent as usize, end as usize));
        }
        let Some((mut cursor, end)) = aperture else {
            continue;
        };
        if crate::machine::iommu::interrupt::overlaps(cursor, end)
            || allocated.iter().any(|&(a, b)| cursor < b && a < end)
            || topology
                .configurations()
                .any(|(base, size)| cursor < base + size && base < end)
            || topology
                .controllers()
                .any(|base| cursor < base + 4096 && base < end)
        {
            return Err(PendFail::Denied);
        }
        allocated.try_reserve(1).map_err(|_| PendFail::OoM)?;
        allocated.push((cursor, end));
        for slot in 0..32 {
            let rid = slot << 3;
            let Ok(route) = topology.resolve(host, rid) else {
                continue;
            };
            let did = route.device();
            let config_va = mapped(host + (rid as usize) * 4096, 4096)?;
            let config = unsafe { Ecam::new(config_va) };
            let mut endpoint = match Endpoint::discover(config) {
                Ok(endpoint) => endpoint,
                // Unsupported functions stay disabled and their aperture/config
                // remain withheld. No attempt to configure bridges or siblings.
                Err(Error::Absent | Error::Unsupported) => continue,
                Err(_) => return Err(PendFail::Denied),
            };
            cursor = cursor
                .checked_next_multiple_of(endpoint.size())
                .ok_or(PendFail::Denied)?;
            let next = cursor
                .checked_add(endpoint.size())
                .filter(|next| *next <= end)
                .ok_or(PendFail::OoM)?;
            devices.try_reserve(1).map_err(|_| PendFail::OoM)?;
            let va = mapped(cursor, endpoint.size())?;
            let events =
                crate::core::work::unit::life::Life::try_new().map_err(|_| PendFail::OoM)?;
            // Validated immutable route; exclusive aperture allocation, checked
            // against RAM/controls/ECAM and withheld from all Unit boot authorities.
            unsafe { endpoint.place(cursor) }.map_err(|_| PendFail::Denied)?;
            devices.push(Entry {
                host,
                rid,
                did,
                endpoint,
                bar: cursor,
                mapped: va,
                config: config_va,
                events,
                fault: None,
                queue_failed: false,
                dma: None,
            });
            cursor = next;
        }
    }
    log::info!(
        "pci: {} protected endpoints in Kernel custody",
        devices.len()
    );
    Ok(())
}
