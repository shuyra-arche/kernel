use alloc::vec::Vec;
use fdt::node::FdtNode;
use kernel_api::abi::call::pend::PendFail;

use super::region::Region;

/// Platform IOMMU control pages never become externally grantable boot resources.
/// This discovers authority boundaries only; it neither starts nor settles DMA.
pub(crate) fn discover(fdt: &fdt::Fdt) -> Result<Vec<Region>, PendFail> {
    let mut reserved = Vec::new();
    for node in fdt.all_nodes() {
        let platform = compatible(node, "riscv,iommu");
        let controller = platform
            || compatible(node, "riscv,pci-iommu")
            || node.property("#iommu-cells").is_some();
        if !controller {
            continue;
        }
        // Unknown/PCI controllers need their own trusted discovery and configuration
        // path. Never treat an unsupported controller as an ordinary device window.
        if !platform || compatible(node, "riscv,pci-iommu") {
            return Err(PendFail::Denied);
        }
        let regions = node_regions(fdt, node)?;
        reserved
            .try_reserve(regions.len())
            .map_err(|_| PendFail::OutOfMemory)?;
        reserved.extend(regions);
    }
    Ok(reserved)
}

fn compatible(node: FdtNode, name: &str) -> bool {
    node.compatible()
        .is_some_and(|value| value.all().any(|value| value == name))
}

/// Keep interrupt and timer controls private even without an active IOMMU route.
/// Status does not make aliases of a protected register safe to delegate.
pub(super) fn interrupts(fdt: &fdt::Fdt) -> Result<Vec<Region>, PendFail> {
    let mut result = Vec::new();
    for node in fdt.all_nodes() {
        let local = compatible(node, "riscv,cpu-intc");
        let controller = node.property("interrupt-controller").is_some()
            || [
                "riscv,plic0",
                "sifive,plic-1.0.0",
                "riscv,clint0",
                "sifive,clint0",
                "riscv,aclint-mtimer",
                "riscv,aclint-mswi",
                "riscv,aclint-sswi",
            ]
            .into_iter()
            .any(|name| compatible(node, name));
        if !controller || (local && node.property("reg").is_none()) {
            continue;
        }
        let regions = node_regions(fdt, node)?;
        result
            .try_reserve(regions.len())
            .map_err(|_| PendFail::OutOfMemory)?;
        result.extend(regions);
    }
    Ok(result)
}

/// The hardware consumers still require identity coordinates; decode and check
/// their byte/page geometry through the same path as ordinary MMIO admission.
pub(super) fn node_regions(fdt: &fdt::Fdt, node: FdtNode) -> Result<Vec<Region>, PendFail> {
    let parent = super::address::parent(fdt, node).ok_or(PendFail::Denied)?;
    super::address::identity_path(fdt, parent)?;
    super::address::regions(fdt, node)
}
