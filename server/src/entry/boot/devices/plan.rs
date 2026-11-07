use crate::machine::iommu::riscv::topology::Topology;
use alloc::vec::Vec;
use kernel_api::abi::call::pend::PendFail;

use super::ranges;
use super::region::Region;
use super::reserved;

/// Pure boot admission. No authority is published until the whole plan succeeds.
pub(super) struct Plan {
    authorities: Vec<Region>,
}

impl Plan {
    pub fn discover(fdt: &fdt::Fdt, dram_base: usize, dram_size: usize) -> Result<Self, PendFail> {
        let mut reserved = reserved::discover(fdt)?;
        let dram = Region::new(dram_base, dram_size)?;
        if reserved.iter().any(|region| region.overlaps(dram)) {
            return Err(PendFail::Denied);
        }
        reserved.try_reserve(1).map_err(|_| PendFail::OoM)?;
        reserved.push(dram);
        if fdt.all_nodes().any(|node| {
            node.property("iommu-map").is_some()
                || node.property("iommu-map-mask").is_some()
                || node.property("iommus").is_some()
        }) {
            // Kernel owns the shared interrupt controller on this restricted
            // platform. Withhold aliases too; queue routing cannot be delegated.
            for node in fdt.all_nodes().filter(|n| {
                n.compatible().is_some_and(|c| {
                    c.all()
                        .any(|v| matches!(v, "riscv,plic0" | "sifive,plic-1.0.0"))
                })
            }) {
                for region in super::reserved::node_regions(fdt, node)? {
                    if region.overlaps(dram) {
                        return Err(PendFail::Denied);
                    }
                    reserved.try_reserve(1).map_err(|_| PendFail::OoM)?;
                    reserved.push(region);
                }
            }
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
                    reserved.try_reserve(1).map_err(|_| PendFail::OoM)?;
                    reserved.push(region);
                }
            }
            for (base, size) in topology.configurations() {
                let region = Region::new(base, size)?;
                if region.overlaps(dram) {
                    return Err(PendFail::Denied);
                }
                reserved.try_reserve(1).map_err(|_| PendFail::OoM)?;
                reserved.push(region);
            }
        }
        let mut plan = Self {
            authorities: Vec::new(),
        };
        for node in fdt.all_nodes() {
            let stem = node.name.split('@').next().unwrap_or(node.name);
            if matches!(stem, "memory" | "clint") {
                continue;
            }
            if let Some(regions) = node.reg() {
                for region in regions {
                    if let Some(size) = region.size {
                        plan.offer(region.starting_address.addr(), size, &reserved)?;
                    }
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
        // Distinct root authorities must not silently compete for one ledger name.
        if self.authorities.iter().any(|prior| prior.base == base) {
            return Err(PendFail::Denied);
        }
        self.authorities.try_reserve(1).map_err(|_| PendFail::OoM)?;
        self.authorities.push(region);
        Ok(())
    }
}
