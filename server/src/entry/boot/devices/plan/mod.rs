use crate::metal::iommu::riscv::topology::Topology;
use alloc::vec::Vec;
use fdt::{Fdt, node::FdtNode};
use kernel_api::abi::call::pend::PendFail;

use crate::metal::board::tree::{self, mmio as address};
mod ranges;
pub(super) mod reserved;

use address::Region;

impl From<tree::Error> for PendFail {
    fn from(error: tree::Error) -> Self {
        match error {
            tree::Error::Description => Self::Denied,
            tree::Error::OutOfMemory => Self::OutOfMemory,
        }
    }
}

/// Pure boot admission. No authority is published until the whole plan succeeds.
pub(super) struct Plan {
    authorities: Vec<Region>,
}

impl Plan {
    pub fn discover(fdt: &fdt::Fdt, dram_base: usize, dram_size: usize) -> Result<Self, PendFail> {
        let mut reserved = reserved::discover(fdt)?;
        let interrupts = reserved::interrupts(fdt)?;
        reserved
            .try_reserve(interrupts.len())
            .map_err(|_| PendFail::OutOfMemory)?;
        reserved.extend(interrupts);
        let dram = Region::new(dram_base, dram_size)?;
        if reserved.iter().any(|region| region.overlaps(dram)) {
            return Err(PendFail::Denied);
        }
        reserved.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        reserved.push(dram);
        if fdt.all_nodes().any(|node| {
            node.property("iommu-map").is_some()
                || node.property("iommu-map-mask").is_some()
                || node.property("iommus").is_some()
        }) {
            let topology = Topology::discover(fdt).map_err(|_| PendFail::Denied)?;
            // A protected ECAM without protected BAR apertures is not exclusive
            // device custody: another Unit could still program the endpoint.
            for node in fdt
                .all_nodes()
                .filter(|n| n.property("iommu-map").is_some())
            {
                for (base, size) in ranges::memory_windows(fdt, node)? {
                    let region = Region::new(base, size)?;
                    if region.overlaps(dram) {
                        return Err(PendFail::Denied);
                    }
                    reserved.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
                    reserved.push(region);
                }
            }
            for (base, size) in topology.configurations() {
                let region = Region::new(base, size)?;
                if region.overlaps(dram) {
                    return Err(PendFail::Denied);
                }
                reserved.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
                reserved.push(region);
            }
        }
        let mut plan = Self {
            authorities: Vec::new(),
        };
        for node in fdt.all_nodes() {
            if !tree::enabled(fdt, node)? || !ordinary(fdt, node)? {
                continue;
            }
            if tree::property(node, "reg")?.is_some() {
                for region in address::regions(fdt, node)? {
                    plan.offer(region.base, region.size, &reserved)?;
                }
            }
            if ranges::is_pci_host(node) {
                for (base, size) in ranges::memory_windows(fdt, node)? {
                    // Retain the existing rejection of PCI apertures over RAM,
                    // including overlap introduced by page rounding.
                    if Region::new(base, size)?.overlaps(dram) {
                        return Err(PendFail::Denied);
                    }
                    plan.offer(base, size, &reserved)?;
                }
            }
        }
        Ok(plan)
    }

    pub fn authorities(&self) -> &[Region] {
        &self.authorities
    }

    fn offer(&mut self, base: usize, size: usize, reserved: &[Region]) -> Result<(), PendFail> {
        if base == 0 || size == 0 {
            return Ok(());
        }
        let region = Region::new(base, size)?;
        // Withhold the entire source. Splitting would invent new ledger resource
        // names and let existing consumers mistake a fragment for the old window.
        if reserved.iter().any(|reserved| region.overlaps(*reserved)) {
            return Ok(());
        }
        // Distinct roots cannot grant overlapping CPU pages, even under different
        // names or after different bus translations.
        if self.authorities.iter().any(|prior| prior.overlaps(region)) {
            return Err(PendFail::Denied);
        }
        self.authorities
            .try_reserve(1)
            .map_err(|_| PendFail::OutOfMemory)?;
        self.authorities.push(region);
        Ok(())
    }
}

/// CPU IDs, reserved-memory and PCI child encodings are not ordinary MMIO.
/// A bus with zero size cells describes identifiers, not memory apertures.
fn ordinary<'b, 'a>(fdt: &'b Fdt<'a>, node: FdtNode<'b, 'a>) -> Result<bool, PendFail> {
    let mut current = node;
    loop {
        if matches!(
            current.name.split('@').next(),
            Some("cpus" | "reserved-memory" | "memory")
        ) {
            return Ok(false);
        }
        let Some(bus) = tree::parent(fdt, current) else {
            return Ok(true);
        };
        if ranges::is_pci_host(bus) || tree::cells(bus, "#size-cells", 1)? == 0 {
            return Ok(false);
        }
        current = bus;
    }
}
