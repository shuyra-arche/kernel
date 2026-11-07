use alloc::vec::Vec;
use kernel_api::abi::call::pie::PieFail;

use super::{ranges, region::Region, reserved};

/// Pure boot admission. No authority is published until the whole plan succeeds.
pub(super) struct Plan {
    grants: Vec<Region>,
}

impl Plan {
    pub fn discover(fdt: &fdt::Fdt, dram_base: usize, dram_size: usize) -> Result<Self, PieFail> {
        let mut reserved = reserved::discover(fdt)?;
        let dram = Region::new(dram_base, dram_size)?;
        if reserved.iter().any(|region| region.overlaps(dram)) {
            return Err(PieFail::Denied);
        }
        reserved.try_reserve(1).map_err(|_| PieFail::OoM)?;
        reserved.push(dram);
        let mut plan = Self { grants: Vec::new() };
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
                        return Err(PieFail::Denied);
                    }
                    plan.offer(base, size, &reserved)?;
                }
            }
        }
        Ok(plan)
    }

    pub fn grants(&self) -> &[Region] {
        &self.grants
    }

    fn offer(&mut self, base: usize, size: usize, reserved: &[Region]) -> Result<(), PieFail> {
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
        if self.grants.iter().any(|prior| prior.base == base) {
            return Err(PieFail::Denied);
        }
        self.grants.try_reserve(1).map_err(|_| PieFail::OoM)?;
        self.grants.push(region);
        Ok(())
    }
}
