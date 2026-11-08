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

fn cell_count(node: FdtNode, name: &str, default: usize) -> Result<usize, PendFail> {
    match node.property(name) {
        None => Ok(default),
        Some(property) if property.value.len() == 4 => number(property.value),
        Some(_) => Err(PendFail::Denied),
    }
}

fn number(value: &[u8]) -> Result<usize, PendFail> {
    value.iter().try_fold(0usize, |number, byte| {
        number
            .checked_mul(256)
            .and_then(|number| number.checked_add(*byte as usize))
            .ok_or(PendFail::Denied)
    })
}

fn parent_of<'b, 'a>(fdt: &'b fdt::Fdt<'a>, node: FdtNode<'b, 'a>) -> Option<FdtNode<'b, 'a>> {
    // Names have distinct storage in the DTB even when their text is identical.
    fdt.all_nodes().find(|parent| {
        parent
            .children()
            .any(|child| core::ptr::eq(child.name, node.name))
    })
}

fn identity_path<'b, 'a>(fdt: &'b fdt::Fdt<'a>, mut bus: FdtNode<'b, 'a>) -> Result<(), PendFail> {
    while let Some(parent) = parent_of(fdt, bus) {
        // Empty ranges explicitly declares identity. Missing or translated ranges
        // cannot be interpreted as CPU physical coordinates by this adapter.
        if !bus
            .property("ranges")
            .is_some_and(|ranges| ranges.value.is_empty())
        {
            return Err(PendFail::Denied);
        }
        bus = parent;
    }
    Ok(())
}

/// Checked physical control ranges, shared by IOMMU and PLIC reservation.
pub(super) fn node_regions(fdt: &fdt::Fdt, node: FdtNode) -> Result<Vec<Region>, PendFail> {
    let parent = parent_of(fdt, node).ok_or(PendFail::Denied)?;
    identity_path(fdt, parent)?;
    let address = cell_count(parent, "#address-cells", 2)?;
    let size = cell_count(parent, "#size-cells", 1)?;
    if !(1..=2).contains(&address) || !(1..=2).contains(&size) {
        return Err(PendFail::Denied);
    }
    let reg = node.property("reg").ok_or(PendFail::Denied)?.value;
    let width = (address + size) * 4;
    if reg.is_empty() || !reg.len().is_multiple_of(width) {
        return Err(PendFail::Denied);
    }
    let mut reserved = Vec::new();
    reserved
        .try_reserve(reg.len() / width)
        .map_err(|_| PendFail::OutOfMemory)?;
    for entry in reg.chunks_exact(width) {
        let base = number(&entry[..address * 4])?;
        let size = number(&entry[address * 4..])?;
        reserved.push(Region::new(base, size)?);
    }
    Ok(reserved)
}
